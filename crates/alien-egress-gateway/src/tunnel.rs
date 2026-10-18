//! Opaque TCP relay after protocol inspection, with bounded buffers and half-close.
use std::io;

use tokio::net::TcpStream;

use crate::io::Metered;

pub(crate) async fn relay(
    guest: &mut Metered<TcpStream>,
    upstream: &mut TcpStream,
) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        tokio::try_join!(
            linux::copy(&guest.inner, upstream, &guest.counters, true),
            linux::copy(upstream, &guest.inner, &guest.counters, false),
        )?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        tokio::io::copy_bidirectional(guest, upstream).await?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        sync::atomic::Ordering,
    };

    use tokio::{io::Interest, net::TcpStream};

    use crate::io::Counters;

    struct Pipe {
        read: OwnedFd,
        write: OwnedFd,
        capacity: usize,
    }

    impl Pipe {
        fn new() -> io::Result<Self> {
            let mut descriptors = [-1; 2];
            // SAFETY: pipe2 initializes exactly these two descriptors on success.
            if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) }
                == -1
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: successful pipe2 transfers two distinct, newly owned descriptors.
            let (read, write) = unsafe {
                (
                    OwnedFd::from_raw_fd(descriptors[0]),
                    OwnedFd::from_raw_fd(descriptors[1]),
                )
            };
            // Match the old relay's 8 KiB per-direction buffer; Linux rounds to page size.
            // SAFETY: read is a live pipe descriptor and this fcntl command takes an integer.
            let capacity = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETPIPE_SZ, 8192) };
            if capacity == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                read,
                write,
                capacity: capacity as usize,
            })
        }
    }

    fn splice(source: RawFd, destination: RawFd, limit: usize) -> io::Result<usize> {
        // SAFETY: callers retain the socket and pipe owners throughout this syscall. Null
        // offsets are required for these non-seekable descriptors; no Rust memory is exposed.
        let count = unsafe {
            libc::splice(
                source,
                std::ptr::null_mut(),
                destination,
                std::ptr::null_mut(),
                limit,
                libc::SPLICE_F_NONBLOCK,
            )
        };
        if count == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }

    pub(super) async fn copy(
        source: &TcpStream,
        destination: &TcpStream,
        counters: &Counters,
        from_guest: bool,
    ) -> io::Result<()> {
        let pipe = Pipe::new()?;
        loop {
            tokio::task::consume_budget().await;
            source.readable().await?;
            // The pipe is empty here, so WouldBlock can only refer to the source socket.
            let count = match source.try_io(Interest::READABLE, || {
                splice(source.as_raw_fd(), pipe.write.as_raw_fd(), pipe.capacity)
            }) {
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            };
            if count == 0 {
                // SAFETY: destination remains borrowed and live. Only its write half closes;
                // the reverse direction continues until its own EOF, error or cancellation.
                if unsafe { libc::shutdown(destination.as_raw_fd(), libc::SHUT_WR) } == -1 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            if from_guest {
                counters.up.fetch_add(count as u64, Ordering::Relaxed);
                counters.touch();
            }
            let mut pending = count;
            while pending != 0 {
                tokio::task::consume_budget().await;
                destination.writable().await?;
                // The pipe is nonempty, so WouldBlock can only refer to the destination.
                let written = match destination.try_io(Interest::WRITABLE, || {
                    splice(pipe.read.as_raw_fd(), destination.as_raw_fd(), pending)
                }) {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "tunnel write returned zero",
                        ))
                    }
                    Ok(written) => written,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                pending -= written;
                if !from_guest {
                    counters.down.fetch_add(written as u64, Ordering::Relaxed);
                    counters.touch();
                }
            }
        }
    }
}
