use async_task::{Runnable, Task};
use flume::{Receiver, Sender};
use futures_lite::future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{future::Future, panic::catch_unwind, thread};

#[derive(Debug, Clone, Copy)]
enum FutureType {
    High,
    Low,
}

fn spawn_task<F, T>(future: F, order: FutureType) -> Task<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    static HIGH_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> =
        LazyLock::new(|| flume::unbounded::<Runnable>());

    static LOW_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> =
        LazyLock::new(|| flume::unbounded::<Runnable>());

    static HIGH_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let high_num: usize = std::env::var("HIGH_NUM").unwrap().parse().unwrap();
        // high priority queue uses multiple workers
        for _ in 0..high_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    // implementing task stealing, worker steals from low
                    // priority queue
                    match high_receiver
                        .try_recv()
                        .or_else(|_| low_receiver.try_recv())
                    {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
        }
        HIGH_CHANNEL.0.clone()
    });

    static LOW_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let low_num: usize = std::env::var("LOW_NUM").unwrap().parse().unwrap();
        for _ in 0..low_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                // this wastes CPU time since try_recv doesn't block and
                // we check again after sleeping, we can park the thread
                // or use conditional variable and let the sending thread
                // wake the worker threads
                loop {
                    // first try low queue then high queue
                    match low_receiver
                        .try_recv()
                        .or_else(|_| high_receiver.try_recv())
                    {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
        }
        LOW_CHANNEL.0.clone()
    });

    let schedule_high = |runnable| HIGH_QUEUE.send(runnable).unwrap();
    let schedule_low = |runnable| LOW_QUEUE.send(runnable).unwrap();

    let schedule = match order {
        FutureType::High => schedule_high,
        FutureType::Low => schedule_low,
    };

    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();
    return task;
}

struct CounterFuture {
    count: u32,
}

impl Future for CounterFuture {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.count += 1;
        println!("polling with result: {}", self.count);
        std::thread::sleep(Duration::from_secs(1));
        if self.count < 3 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.count)
        }
    }
}

async fn async_fn() {
    std::thread::sleep(Duration::from_secs(1));
    println!("async fn");
}

// macro for spawn task
macro_rules! spawn_task {
    ($future:expr) => {
        spawn_task!($future, FutureType::Low)
    };
    ($future:expr, $order: expr) => {
        spawn_task($future, $order)
    };
}
// macro for joining or blocking on tasks
macro_rules! join {
    ($($future:expr),*) => {
        let mut results = Vec::new();
        $(
            results.push(future::block_on($future));
        )*
    };
}

// join with error handling, returns Result type
macro_rules! try_join {
    ($($future:expr),*) => {
        let mut results = Vec::new();
        $(
            results.push(catch_unwind(|| future::block_on($future)));
        )*
    };
}

struct Runtime {
    high_num: usize,
    low_num: usize,
}

impl Runtime {
    pub fn new() -> Self {
        let num_cores = std::thread::available_parallelism().unwrap().get();
        Self {
            high_num: num_cores - 2,
            low_num: 1,
        }
    }

    pub fn with_high_num(mut self, val: usize) -> Self {
        self.high_num = val;
        self
    }

    pub fn with_low_num(mut self, val: usize) -> Self {
        self.low_num = val;
        self
    }

    pub fn run(&self) {
        std::env::set_var("HIGH_NUM", self.high_num.to_string());
        std::env::set_var("LOW_NUM", self.low_num.to_string());

        // initialize the queues by sending an empty future
        let high = spawn_task!(async {}, FutureType::High);
        let low = spawn_task!(async {}, FutureType::Low);
        join!(high, low);
    }
}

#[derive(Debug, Clone, Copy)]
struct BackgroundProcess;

impl Future for BackgroundProcess {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("background process firing");
        std::thread::sleep(Duration::from_secs(1));
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

fn main() {
    Runtime::new().with_low_num(2).with_high_num(4).run();
    spawn_task!(BackgroundProcess).detach();
    let one = CounterFuture { count: 0 };
    let two = CounterFuture { count: 0 };
    let t_one = spawn_task!(one, FutureType::High);
    let t_two = spawn_task!(two);
    let t_three = spawn_task!(async_fn());
    let t_four = spawn_task!(
        async {
            async_fn().await;
            async_fn().await;
        },
        FutureType::High
    );

    std::thread::sleep(Duration::from_secs(5));
    println!("before the block");
    // join only futures with the same output
    join!(t_one, t_two);
    try_join!(t_three, t_four);
}
