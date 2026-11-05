use std::{
    collections::HashMap,
    future::poll_fn,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

use bevy_asset::io::{AssetReader, AssetReaderError, PathStream, Reader};

#[derive(Default)]
struct Gate {
    open: bool,
    waiters: Vec<Waker>,
}

#[derive(Clone, Default)]
pub struct GateOpener(Arc<Mutex<Gates>>);

#[derive(Default)]
struct Gates {
    all_open: bool,
    paths: HashMap<PathBuf, Gate>,
}

impl GateOpener {
    pub fn open(&self, path: impl AsRef<Path>) {
        let waiters = {
            let mut gates = self.0.lock().unwrap();
            let gate = gates.paths.entry(path.as_ref().to_owned()).or_default();
            gate.open = true;
            std::mem::take(&mut gate.waiters)
        };
        for waker in waiters {
            waker.wake();
        }
    }

    pub fn open_all(&self) {
        let paths = {
            let mut gates = self.0.lock().unwrap();
            gates.all_open = true;
            std::mem::take(&mut gates.paths)
        };
        for gate in paths.into_values() {
            for waker in gate.waiters {
                waker.wake();
            }
        }
    }
}

#[derive(Clone)]
pub struct GatedReader<R> {
    reader: R,
    gates: GateOpener,
}

impl<R> GatedReader<R> {
    pub fn new(reader: R) -> (Self, GateOpener) {
        let gates = GateOpener::default();
        (
            Self {
                reader,
                gates: gates.clone(),
            },
            gates,
        )
    }
}

impl<R: AssetReader> AssetReader for GatedReader<R> {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        poll_fn(|context| {
            let mut gates = self.gates.0.lock().unwrap();
            if gates.all_open {
                return Poll::Ready(());
            }
            let gate = gates.paths.entry(path.to_owned()).or_default();
            if gate.open {
                Poll::Ready(())
            } else {
                if !gate
                    .waiters
                    .iter()
                    .any(|waker| waker.will_wake(context.waker()))
                {
                    gate.waiters.push(context.waker().clone());
                }
                Poll::Pending
            }
        })
        .await;
        self.reader.read(path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.reader.read_meta(path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.reader.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.reader.is_directory(path).await
    }
}
