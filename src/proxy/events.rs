use crossbeam_channel::{unbounded, Receiver, Sender};

/// Events emitted by the proxy for observation (Phase 2 hooks).
///
/// In Phase 1, the receiver is immediately dropped — `try_send` silently
/// fails with zero overhead. Phase 2 keeps the receiver alive for UI hooks.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 2 — variants constructed in tests, used by Tauri GUI
pub enum ProxyEvent {
    SyncBlockComplete {
        size_bytes: usize,
        is_full_redraw: bool,
    },
    RenderComplete {
        output_bytes: usize,
        diff_bytes: usize,
        frame_number: u64,
    },
    Resize {
        cols: i16,
        rows: i16,
    },
    ChildExited {
        exit_code: u32,
    },
}

/// Create a proxy event channel.
pub fn event_channel() -> (Sender<ProxyEvent>, Receiver<ProxyEvent>) {
    unbounded()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_variants_constructible() {
        let _sync = ProxyEvent::SyncBlockComplete {
            size_bytes: 1024,
            is_full_redraw: true,
        };
        let _render = ProxyEvent::RenderComplete {
            output_bytes: 512,
            diff_bytes: 128,
            frame_number: 1,
        };
        let _resize = ProxyEvent::Resize {
            cols: 120,
            rows: 40,
        };
        let _exit = ProxyEvent::ChildExited { exit_code: 0 };
    }

    #[test]
    fn test_dropped_receiver_does_not_block() {
        let (tx, rx) = event_channel();
        drop(rx); // Drop receiver

        // try_send should fail silently, not block or panic
        let result = tx.try_send(ProxyEvent::ChildExited { exit_code: 0 });
        assert!(result.is_err());
    }

    #[test]
    fn test_channel_sends_and_receives() {
        let (tx, rx) = event_channel();
        tx.send(ProxyEvent::Resize {
            cols: 80,
            rows: 24,
        })
        .unwrap();

        let event = rx.recv().unwrap();
        match event {
            ProxyEvent::Resize { cols, rows } => {
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("expected Resize event"),
        }
    }

    #[test]
    fn test_event_is_clone() {
        let event = ProxyEvent::RenderComplete {
            output_bytes: 100,
            diff_bytes: 50,
            frame_number: 42,
        };
        let cloned = event.clone();
        match cloned {
            ProxyEvent::RenderComplete { frame_number, .. } => {
                assert_eq!(frame_number, 42);
            }
            _ => panic!("clone changed variant"),
        }
    }
}
