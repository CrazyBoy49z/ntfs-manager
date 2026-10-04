use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

use anyhow::{bail, Context, Result};

use crate::helper_protocol::{
    HelperRequest, HelperResponse, HELPER_SOCKET, MAX_MESSAGE_BYTES,
};

#[derive(Clone, Debug)]
pub struct HelperClient {
    socket_path: String,
}

impl Default for HelperClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HelperClient {
    pub fn new() -> Self {
        Self {
            socket_path: HELPER_SOCKET.to_string(),
        }
    }

    pub fn ping(&self) -> Result<()> {
        self.request(&HelperRequest::Ping).map(|_| ())
    }

    pub fn mount(&self, device: &str, mount_point: Option<&Path>) -> Result<String> {
        let response = self.request(&HelperRequest::Mount {
            device: device.to_string(),
            mount_point: mount_point.map(Path::to_path_buf),
        })?;

        response
            .mount_point
            .context("helper returned success without a mount point")
    }

    pub fn unmount(&self, device: &str) -> Result<()> {
        self.request(&HelperRequest::Unmount {
            device: device.to_string(),
        })
        .map(|_| ())
    }

    fn request(&self, request: &HelperRequest) -> Result<HelperResponse> {
        let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "cannot connect to privileged helper at {}; run the installer or start the launch daemon",
                self.socket_path
            )
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        serde_json::to_writer(&mut stream, request)?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        let mut line = String::new();
        let mut reader = BufReader::new(stream).take(MAX_MESSAGE_BYTES);
        reader.read_line(&mut line)?;

        if line.trim().is_empty() {
            bail!("privileged helper closed the connection without a response");
        }

        let response: HelperResponse =
            serde_json::from_str(&line).context("invalid response from privileged helper")?;

        if !response.ok {
            bail!("{}", response.message);
        }

        Ok(response)
    }
}
