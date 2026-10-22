//! Node.js IPC transport (`--node-ipc`), as used by `vscode-languageclient`
//! with `TransportKind.ipc`: the parent passes a channel file descriptor in
//! `NODE_CHANNEL_FD`; every message is one line of JSON.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};

use lsp_server::Connection;
#[cfg(unix)]
use lsp_server::Message;

/// Creates a connection over the Node IPC channel of this process.
#[cfg(unix)]
pub fn connect() -> std::io::Result<(Connection, std::thread::JoinHandle<()>)> {
    use std::os::fd::FromRawFd;
    let fd: i32 = std::env::var("NODE_CHANNEL_FD")
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| std::io::Error::other("NODE_CHANNEL_FD is not set"))?;
    // SAFETY: the parent process handed us this descriptor for exclusive use.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut writer = file.try_clone()?;
    let (reader_tx, reader_rx) = crossbeam_channel::unbounded::<Message>();
    let (writer_tx, writer_rx) = crossbeam_channel::unbounded::<Message>();
    std::thread::spawn(move || {
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Message>(&line) {
                Ok(msg) => {
                    let is_exit = matches!(&msg, Message::Notification(n) if n.method == "exit");
                    if reader_tx.send(msg).is_err() || is_exit {
                        break;
                    }
                }
                Err(e) => eprintln!("php-lsp: invalid IPC message: {e}"),
            }
        }
    });
    let handle = std::thread::spawn(move || {
        for msg in writer_rx {
            let Ok(mut json) = serde_json::to_string(&msg) else { continue };
            json.push('\n');
            if writer.write_all(json.as_bytes()).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });
    Ok((Connection { sender: writer_tx, receiver: reader_rx }, handle))
}

#[cfg(not(unix))]
pub fn connect() -> std::io::Result<(Connection, std::thread::JoinHandle<()>)> {
    Err(std::io::Error::other("--node-ipc is only supported on Unix; use --stdio"))
}
