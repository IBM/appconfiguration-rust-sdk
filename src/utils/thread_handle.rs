// (C) Copyright IBM Corp. 2025.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadStatus<ResultType> {
    Running,
    Finished(ResultType),
    FailedInternalError(String),
}

/// Wrapper around a thread that offers the following capabilities:
/// * The thread is terminated when the object goes out of scope
/// * A `get_thread_status` method to check if thread is running or
///   finished with a specific result
#[derive(Debug)]
pub(crate) struct ThreadHandle<ResultType: Send + Clone + 'static> {
    pub(crate) _thread_termination_sender: std::sync::mpsc::Sender<()>,
    pub(crate) thread_handle: Option<JoinHandle<ResultType>>,
    pub(crate) finished_thread_status_cached: Option<ThreadStatus<ResultType>>,
}

impl<ResultType: Send + Clone + 'static> ThreadHandle<ResultType> {
    pub(crate) fn new<F>(f: F) -> Self
    where
        F: FnOnce(Receiver<()>) -> ResultType,
        F: Send + 'static,
    {
        let (thread_termination_sender, thread_termination_receiver) = std::sync::mpsc::channel();

        let t: JoinHandle<ResultType> = std::thread::spawn(move || f(thread_termination_receiver));

        Self {
            _thread_termination_sender: thread_termination_sender,
            thread_handle: Some(t),
            finished_thread_status_cached: None,
        }
    }

    pub(crate) fn get_thread_status(&mut self) -> ThreadStatus<ResultType> {
        let t = self.thread_handle.take();
        match t {
            Some(t) => {
                if t.is_finished() {
                    let thread_finished_status = match t.join() {
                        Ok(r) => ThreadStatus::Finished(r),
                        Err(e) => {
                            if let Some(panic_msg) = e.downcast_ref::<String>() {
                                ThreadStatus::FailedInternalError(format!(
                                    "Thread panicked: {}",
                                    panic_msg
                                ))
                            } else if let Some(panic_msg) = e.downcast_ref::<&str>() {
                                ThreadStatus::FailedInternalError(format!(
                                    "Thread panicked: {}",
                                    panic_msg
                                ))
                            } else {
                                ThreadStatus::FailedInternalError("Thread panicked".to_string())
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
    use std::sync::mpsc::RecvError;

    use super::*;

    #[test]
    fn neverending_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut handle = ThreadHandle::new(move |terminator| {
            terminator.recv().unwrap();
            tx.send(()).unwrap();
            Ok::<(), String>(())
        });

        assert_eq!(handle.get_thread_status(), ThreadStatus::Running);
        assert_eq!(handle.get_thread_status(), ThreadStatus::Running);

        drop(handle);
        assert_eq!(rx.recv().unwrap_err(), RecvError);
    }

    #[test]
    fn finishing_thread() {
        let mut handle = ThreadHandle::new(move |_terminator| Ok::<(), String>(()));
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(handle.get_thread_status(), ThreadStatus::Finished(Ok(())));
        assert_eq!(handle.get_thread_status(), ThreadStatus::Finished(Ok(())));
    }

    #[test]
    fn finishing_thread_with_error() {
        let mut handle = ThreadHandle::new(move |_terminator| Err::<(), _>("I failed".to_string()));
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::Finished(Err("I failed".to_string()))
        );
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::Finished(Err("I failed".to_string()))
        );
    }

    #[test]
    fn panicking_thread() {
        let mut handle = ThreadHandle::new(move |_terminator| {
            panic!("panic for test");
        });
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::FailedInternalError("Thread panicked: panic for test".to_string())
        );
        assert_eq!(
            handle.get_thread_status(),
            ThreadStatus::FailedInternalError("Thread panicked: panic for test".to_string())
        );
    }
}
