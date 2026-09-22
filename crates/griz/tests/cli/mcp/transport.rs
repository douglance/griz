//! A bounded receive path for MCP test responses.

use std::{
    io::{BufRead, BufReader},
    process::ChildStdout,
    sync::mpsc::{Receiver, channel},
};

pub(super) fn lines(stdout: ChildStdout) -> Receiver<std::io::Result<String>> {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let _ = BufReader::new(stdout)
            .lines()
            .try_for_each(|line| sender.send(line));
    });
    receiver
}
