use std::{sync::mpsc::Receiver, thread::JoinHandle};

use super::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadStatus {
    Running,
    Finished(Result<()>),
}

pub(crate) struct ThreadHandle {
    _thread_termination_sender: std::sync::mpsc::Sender<()>,
    thread_handle: Option<JoinHandle<Result<()>>>,
    finished_thread_status_cached: Option<ThreadStatus>,
}

impl ThreadHandle {
    pub(crate) fn new<F>(f: F) -> Self
    where
        F: FnOnce(Receiver<()>) -> Result<()>,
        F: Send + 'static,
    {
        let (thread_termination_sender, thread_termination_receiver) = std::sync::mpsc::channel();

        let t: JoinHandle<Result<()>> = std::thread::spawn(move || f(thread_termination_receiver));

        Self {
            _thread_termination_sender: thread_termination_sender,
            thread_handle: Some(t),
            finished_thread_status_cached: None,
        }
    }

    pub(crate) fn get_thread_status(&mut self) -> ThreadStatus {
        let t = self.thread_handle.take();
        match t {
            Some(t) => {
                if t.is_finished() {
                    let thread_finished_status =
                        match t.join() {
                            Ok(r) => ThreadStatus::Finished(r),
                            Err(e) => {
                                if let Some(panic_msg) = e.downcast_ref::<String>() {
                                    ThreadStatus::Finished(Err(Error::ThreadInternalError(
                                        format!("Thread panicked: {}", panic_msg),
                                    )))
                                } else if let Some(panic_msg) = e.downcast_ref::<&str>() {
                                    ThreadStatus::Finished(Err(Error::ThreadInternalError(
                                        format!("Thread panicked: {}", panic_msg),
                                    )))
                                } else {
                                    ThreadStatus::Finished(Err(Error::ThreadInternalError(
                                        "Thread panicked".to_string(),
                                    )))
                                }
                            }
                        };
                    self.finished_thread_status_cached = Some(thread_finished_status.clone());
                    thread_finished_status
                } else {
                    self.thread_handle = Some(t);
                    ThreadStatus::Running
                }
            }
            None => match &self.finished_thread_status_cached {
                Some(s) => s.clone(),
                None => unreachable!(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc::{RecvError, Sender}, thread::sleep, time::Duration};

    use crate::network::configuration_sync::thread_handle::ThreadStatus;

    use super::ThreadHandle;
    use crate::network::configuration_sync::Error::ThreadInternalError;

    #[test]
    fn neverending_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut handle = ThreadHandle::new(move |terminator| {
            terminator.recv().unwrap();
            tx.send(()).unwrap();
            Ok(())
        });

        assert_eq!(handle.get_thread_status(), ThreadStatus::Running);
        assert_eq!(handle.get_thread_status(), ThreadStatus::Running);

        drop(handle);
        assert_eq!(rx.recv().unwrap_err(), RecvError);
    }

    #[test]
    fn finishing_thread() {
        let mut handle = ThreadHandle::new(move |terminator| {
            Ok(())
        });
        sleep(Duration::from_millis(5));
        assert_eq!(handle.get_thread_status(), ThreadStatus::Finished(Ok(())));
        assert_eq!(handle.get_thread_status(), ThreadStatus::Finished(Ok(())));
    }

    #[test]
    fn panicking_thread() {
        let mut handle = ThreadHandle::new(move |terminator| {
            panic!("panic for test");
        });
        sleep(Duration::from_millis(5));
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::Finished(Err(ThreadInternalError("Thread panicked: panic for test".to_string())))
        );
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::Finished(Err(ThreadInternalError("Thread panicked: panic for test".to_string())))
        );
    }
}
