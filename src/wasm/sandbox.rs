//! Wasmtime sandbox for secure WASM execution.

use std::time::Duration;

use wasmtime::{Caller, Config, Engine, Linker, Module, Store, Val};

use super::host::{HostCapabilities, HostState, LogLevel};
use super::WasmError;

/// Compiled WASM module.
pub struct CompiledModule {
    module: Module,
}

impl CompiledModule {
    fn new(module: Module) -> Self {
        Self { module }
    }
}

/// Result from WASM execution.
pub struct ExecutionResult {
    pub value: serde_json::Value,
    pub fuel_consumed: u64,
    pub memory_peak_bytes: u64,
}

/// Sandbox configuration.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory in bytes
    pub max_memory_bytes: u64,
    /// Fuel limit (execution steps)
    pub fuel_limit: u64,
    /// Execution timeout
    pub timeout: Duration,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            fuel_limit: 100_000_000,
            timeout: Duration::from_secs(60),
        }
    }
}

/// WASM sandbox using Wasmtime.
pub struct WasmSandbox {
    engine: Engine,
    config: SandboxConfig,
}

impl WasmSandbox {
    /// Create a new sandbox.
    pub fn new(config: SandboxConfig) -> Result<Self, WasmError> {
        let mut wasmtime_config = Config::new();

        // Enable fuel metering for resource limits
        wasmtime_config.consume_fuel(true);

        // Set memory limits
        wasmtime_config.max_wasm_stack(512 * 1024); // 512KB stack

        // Enable WASI for basic functionality
        wasmtime_config.wasm_component_model(true);

        let engine = Engine::new(&wasmtime_config)
            .map_err(|e| WasmError::CompilationFailed(e.to_string()))?;

        Ok(Self { engine, config })
    }

    /// Compile WASM bytes into a module.
    pub fn compile(&self, wasm_bytes: &[u8]) -> Result<CompiledModule, WasmError> {
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| WasmError::CompilationFailed(e.to_string()))?;

        Ok(CompiledModule::new(module))
    }

    /// Execute a function in a compiled module.
    pub fn execute(
        &self,
        module: &CompiledModule,
        function: &str,
        params: serde_json::Value,
        capabilities: HostCapabilities,
    ) -> Result<ExecutionResult, WasmError> {
        // Create host state with input params serialized into memory
        let params_json = serde_json::to_string(&params)
            .map_err(|e| WasmError::ExecutionFailed(format!("Failed to serialize params: {e}")))?;
        let mut host_state = HostState::new(capabilities);
        host_state.input_json = Some(params_json);

        // Create store with fuel
        let mut store = Store::new(&self.engine, host_state);
        store
            .set_fuel(self.config.fuel_limit)
            .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // Create linker and add host functions
        let mut linker = Linker::new(&self.engine);

        // host_log(level: i32, ptr: i32, len: i32)
        linker.func_wrap("env", "host_log", |mut caller: Caller<'_, HostState>, level: i32, ptr: i32, len: i32| {
            let memory = caller.get_export("memory")
                .and_then(|e| e.into_memory());
            if let Some(memory) = memory {
                let data = memory.data(&caller);
                let start = ptr as usize;
                let end = start + len as usize;
                if end <= data.len() {
                    let msg = std::str::from_utf8(&data[start..end])
                        .map(|s| s.to_string())
                        .ok();
                    if let Some(msg) = msg {
                        let log_level = match level {
                            0 => LogLevel::Debug,
                            1 => LogLevel::Info,
                            2 => LogLevel::Warn,
                            _ => LogLevel::Error,
                        };
                        caller.data_mut().log(log_level, msg);
                    }
                }
            }
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // host_time_now() -> i64 (millis since epoch)
        linker.func_wrap("env", "host_time_now", |caller: Caller<'_, HostState>| -> i64 {
            if caller.data().capabilities.clock_access {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            } else {
                0
            }
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // host_random(ptr: i32, len: i32) -> i32 (0=ok, -1=denied)
        linker.func_wrap("env", "host_random", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
            if !caller.data().capabilities.random_access {
                return -1;
            }
            let memory = caller.get_export("memory")
                .and_then(|e| e.into_memory());
            if let Some(memory) = memory {
                let start = ptr as usize;
                let end = start + len as usize;
                let data = memory.data_mut(&mut caller);
                if end <= data.len() {
                    use rand::RngCore;
                    rand::thread_rng().fill_bytes(&mut data[start..end]);
                    return 0;
                }
            }
            -1
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // host_get_input_len() -> i32
        linker.func_wrap("env", "host_get_input_len", |caller: Caller<'_, HostState>| -> i32 {
            caller.data().input_json.as_ref().map(|s| s.len() as i32).unwrap_or(0)
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // host_get_input(ptr: i32, len: i32) -> i32 (bytes written)
        linker.func_wrap("env", "host_get_input", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
            let json = caller.data().input_json.clone().unwrap_or_default();
            let bytes = json.as_bytes();
            let to_copy = (len as usize).min(bytes.len());
            let memory = caller.get_export("memory")
                .and_then(|e| e.into_memory());
            if let Some(memory) = memory {
                let start = ptr as usize;
                let data = memory.data_mut(&mut caller);
                if start + to_copy <= data.len() {
                    data[start..start + to_copy].copy_from_slice(&bytes[..to_copy]);
                    return to_copy as i32;
                }
            }
            -1
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // host_set_output(ptr: i32, len: i32)
        linker.func_wrap("env", "host_set_output", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
            let memory = caller.get_export("memory")
                .and_then(|e| e.into_memory());
            if let Some(memory) = memory {
                let data = memory.data(&caller);
                let start = ptr as usize;
                let end = start + len as usize;
                if end <= data.len() {
                    if let Ok(output) = std::str::from_utf8(&data[start..end]) {
                        caller.data_mut().output_json = Some(output.to_string());
                    }
                }
            }
        }).map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // Instantiate module
        let instance = linker
            .instantiate(&mut store, &module.module)
            .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

        // Call the target function
        let func = instance
            .get_func(&mut store, function)
            .ok_or_else(|| WasmError::ExecutionFailed(format!("Function '{function}' not found in module")))?;

        let func_ty = func.ty(&store);
        let param_count = func_ty.params().len();
        let result_count = func_ty.results().len();

        // Call with no params (tool convention: use host_get_input/host_set_output)
        let mut results = vec![Val::I32(0); result_count];
        if param_count == 0 {
            func.call(&mut store, &[], &mut results)
                .map_err(|e| {
                    // Check if it's a fuel exhaustion
                    let msg = e.to_string();
                    if msg.contains("fuel") {
                        WasmError::FuelExhausted(self.config.fuel_limit)
                    } else {
                        WasmError::ExecutionFailed(msg)
                    }
                })?;
        } else {
            return Err(WasmError::ExecutionFailed(format!(
                "Function '{function}' expects {param_count} params; use host_get_input for parameter passing"
            )));
        }

        let fuel_remaining = store.get_fuel()
            .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;
        let fuel_consumed = self.config.fuel_limit - fuel_remaining;

        // Get memory peak
        let memory_peak = instance
            .get_memory(&mut store, "memory")
            .map(|m| m.data_size(&store) as u64)
            .unwrap_or(0);

        // Extract output from host state or return code
        let output = store.data().output_json.clone();
        let value = if let Some(json_str) = output {
            serde_json::from_str(&json_str).unwrap_or(serde_json::Value::String(json_str))
        } else {
            // Use the return value if it's an i32
            let return_val = results.first()
                .and_then(|v| v.i32())
                .unwrap_or(0);
            serde_json::json!({ "return_code": return_val })
        };

        Ok(ExecutionResult {
            value,
            fuel_consumed,
            memory_peak_bytes: memory_peak,
        })
    }

    /// Get engine reference.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_creation() {
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();
        // Just verify it was created successfully
        let _ = sandbox.engine();
    }

    #[test]
    fn test_compile_invalid_wasm() {
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();

        let result = sandbox.compile(&[0, 1, 2, 3]); // Invalid WASM
        assert!(matches!(result, Err(WasmError::CompilationFailed(_))));
    }

    #[test]
    fn test_compile_minimal_wasm() {
        // Minimal valid WASM module (empty)
        let wasm = wat::parse_str(r#"(module)"#).unwrap();
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();
        assert!(sandbox.compile(&wasm).is_ok());
    }

    #[test]
    fn test_execute_simple_function() {
        // WASM module with a simple function that returns 42
        let wasm = wat::parse_str(r#"
            (module
                (func (export "run") (result i32)
                    i32.const 42
                )
            )
        "#).unwrap();

        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();
        let module = sandbox.compile(&wasm).unwrap();

        let result = sandbox.execute(
            &module,
            "run",
            serde_json::json!({}),
            HostCapabilities::minimal(),
        ).unwrap();

        assert_eq!(result.value["return_code"], 42);
        assert!(result.fuel_consumed > 0);
    }
}
