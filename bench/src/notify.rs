use crate::client::Client;
use crate::config::Config;
use ckb_core::block::Block;
use ckb_core::BlockNumber;
use ckb_logger::debug;
use ckb_util::Mutex;
use crossbeam_channel::{Receiver, Sender};
use failure::Error;
use std::ops::Deref;
use std::sync::Arc;
use std::thread::{sleep, spawn, JoinHandle};
use std::time::Duration;

pub struct Notifier {
    safe_window: u64,
    client: Client,
    tip: Arc<Mutex<BlockNumber>>,
    subscribers: Vec<Sender<Arc<Block>>>,
}

impl Deref for Notifier {
    type Target = Client;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl Notifier {
    pub fn init(config: &Config) -> Result<Self, Error> {
        let client = Client::init(config)?;
        let tip = Arc::new(Mutex::new(client.get_safe_tip_header().number()));
        Ok(Self {
            safe_window: config.safe_window,
            client,
            tip,
            subscribers: Vec::new(),
        })
    }

    pub fn tip(&self) -> &Arc<Mutex<BlockNumber>> {
        &self.tip
    }

    pub fn subscribe(&mut self) -> Receiver<Arc<Block>> {
        let (sender, receiver) = crossbeam_channel::unbounded();
        self.subscribers.push(sender);
        receiver
    }

    pub fn spawn_watch(self, mut current: BlockNumber) -> JoinHandle<()> {
        let current_tip = self.client.get_safe_tip_header().number();
        *self.tip.lock() = current_tip;
        while current + self.safe_window <= current_tip {
            let block: Block = self
                .client
                .get_block_by_number(current)
                .expect("safe checked")
                .into();
            self.publish(block);
            current += 1;
        }

        spawn(move || loop {
            let safe_number = self.client.get_safe_tip_header().number();
            if safe_number > current_tip {
                *self.tip.lock() = safe_number;
            }

            if current + self.safe_window > safe_number {
                sleep(Duration::from_millis(100));
                continue;
            }

            while current + self.safe_window <= safe_number {
                let block: Block = self
                    .client
                    .get_block_by_number(current)
                    .expect("safe checked")
                    .into();
                debug!(
                    "publish block #{} {:#x}, contains {} transactions",
                    block.header().number(),
                    block.header().hash(),
                    block.transactions().len(),
                );
                self.publish(block);
                current += 1;
            }
        })
    }

    fn publish(&self, block: Block) {
        let block = Arc::new(block);
        self.subscribers.iter().for_each(|subscriber| {
            subscriber.send(Arc::clone(&block)).expect("publish block");
        });
    }
}
