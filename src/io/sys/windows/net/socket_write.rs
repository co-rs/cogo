use std::io;
use std::os::windows::io::{AsRawSocket, FromRawSocket, IntoRawSocket, RawSocket};
use std::time::Duration;

use super::super::{co_io_result, EventData};
use crate::coroutine_impl::{CoroutineImpl, EventSource};
use crate::scheduler::get_scheduler;
use miow::net::TcpStreamExt;

use windows_sys::Win32::Foundation::HANDLE;

pub struct SocketWrite<'a> {
    io_data: EventData,
    buf: &'a [u8],
    socket: RawSocket,
    timeout: Option<Duration>,
}

impl<'a> SocketWrite<'a> {
    pub fn new<T: AsRawSocket>(s: &T, buf: &'a [u8], timeout: Option<Duration>) -> Self {
        let socket = s.as_raw_socket();
        SocketWrite {
            io_data: EventData::new(socket as HANDLE),
            buf,
            socket,
            timeout,
        }
    }

    pub fn done(&mut self) -> io::Result<usize> {
        co_io_result(&self.io_data)
    }
}

impl<'a> EventSource for SocketWrite<'a> {
    #[allow(clippy::needless_return)]
    fn subscribe(&mut self, co: CoroutineImpl) {
        let s = get_scheduler();
        if let Some(dur) = self.timeout {
            s.get_selector().add_io_timer(&mut self.io_data, dur);
        }

        // prepare the co first
        self.io_data.co = Some(co);
        // call the overlapped write API
        co_try!(s, self.io_data.co.take().expect("can't get co"), unsafe {
            let socket: std::net::TcpStream = FromRawSocket::from_raw_socket(self.socket);
            let ret = socket.write_overlapped(self.buf, self.io_data.get_overlapped());
            // don't close the socket
            socket.into_raw_socket();
            ret
        });
    }
}
