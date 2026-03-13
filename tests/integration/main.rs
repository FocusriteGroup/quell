// Integration tests for ConPTY session management
//
// Run with: cargo test --test integration

#[cfg(windows)]
mod conpty_tests {
    use std::time::{Duration, Instant};

    fn conpty_run(command: &str, inputs: &[&str], timeout: Duration) -> String {
        use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
        use windows::Win32::System::Console::{
            ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON,
        };
        use windows::Win32::System::Pipes::CreatePipe;
        use windows::Win32::System::Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
            CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
        };
        use std::mem;

        const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x00020016;

        unsafe {
            let mut input_read = HANDLE::default();
            let mut input_write = HANDLE::default();
            CreatePipe(&mut input_read, &mut input_write, None, 0).unwrap();

            let mut output_read = HANDLE::default();
            let mut output_write = HANDLE::default();
            CreatePipe(&mut output_read, &mut output_write, None, 0).unwrap();

            let size = COORD { X: 120, Y: 30 };
            let hpc = CreatePseudoConsole(size, input_read, output_write, 0).unwrap();

            // Close ConPTY-side handles (ConPTY duplicated them)
            let _ = CloseHandle(input_read);
            let _ = CloseHandle(output_write);

            let mut attr_size: usize = 0;
            let _ = InitializeProcThreadAttributeList(None, 1, Some(0), &mut attr_size);
            let mut attr_buf: Vec<u8> = vec![0; attr_size];
            let attr_list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(Some(attr_list), 1, Some(0), &mut attr_size).unwrap();

            // Pass the raw HPCON handle value (not a pointer to the variable)
            let hpc_raw = hpc.0 as *const std::ffi::c_void;
            UpdateProcThreadAttribute(
                attr_list, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                Some(hpc_raw), mem::size_of::<HPCON>(), None, None,
            ).unwrap();

            let mut cmd_wide: Vec<u16> = command.encode_utf16().collect();
            cmd_wide.push(0);

            let mut si: STARTUPINFOEXW = mem::zeroed();
            si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
            si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
            si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
            si.lpAttributeList = attr_list;

            let mut pi = PROCESS_INFORMATION::default();
            CreateProcessW(
                None, Some(windows::core::PWSTR(cmd_wide.as_mut_ptr())),
                None, None, false,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                None, None, &si.StartupInfo, &mut pi,
            ).unwrap();

            DeleteProcThreadAttributeList(attr_list);

            // Start reader in background
            let read_ptr = output_read.0 as usize;
            let reader = std::thread::spawn(move || {
                let handle = windows::Win32::Foundation::HANDLE(read_ptr as *mut _);
                let mut buf = [0u8; 4096];
                let mut collected = Vec::new();
                loop {
                    let mut n = 0u32;
                    match ReadFile(handle, Some(&mut buf), Some(&mut n), None) {
                        Ok(()) if n > 0 => collected.extend_from_slice(&buf[..n as usize]),
                        _ => break,
                    }
                }
                collected
            });

            // Write inputs
            for input in inputs {
                std::thread::sleep(Duration::from_millis(500));
                let bytes = input.as_bytes();
                let mut n = 0u32;
                let _ = WriteFile(input_write, Some(bytes), Some(&mut n), None);
            }

            // Wait for child, then close ConPTY to break output pipe
            WaitForSingleObject(pi.hProcess, timeout.as_millis() as u32);
            ClosePseudoConsole(hpc);

            let output = reader.join().unwrap_or_default();

            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(input_write);
            let _ = CloseHandle(output_read);

            String::from_utf8_lossy(&output).to_string()
        }
    }

    #[test]
    fn test_echo_roundtrip() {
        let output = conpty_run(
            "cmd.exe /c echo hello world",
            &[],
            Duration::from_secs(5),
        );
        assert!(
            output.contains("hello world"),
            "expected 'hello world' in output (len={}), got: {:?}",
            output.len(),
            &output[..output.len().min(500)]
        );
    }

    #[test]
    fn test_child_exit_on_completion() {
        let start = Instant::now();
        let _output = conpty_run("cmd.exe /c echo done", &[], Duration::from_secs(5));
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "took too long: {:?}", start.elapsed()
        );
    }

    #[test]
    fn test_interactive_echo() {
        let output = conpty_run(
            "cmd.exe",
            &["echo test123\r\n", "exit\r\n"],
            Duration::from_secs(10),
        );
        assert!(
            output.contains("test123"),
            "expected 'test123' in output (len={}), got: {:?}",
            output.len(),
            &output[..output.len().min(500)]
        );
    }

    #[test]
    fn test_large_output() {
        let output = conpty_run(
            "cmd.exe /c \"for /L %i in (1,1,100) do @echo line%i\"",
            &[],
            Duration::from_secs(10),
        );
        let count = output.matches("line").count();
        assert!(count >= 50, "expected >= 50 'line' matches, got {count}");
    }
}
