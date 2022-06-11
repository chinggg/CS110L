use crossbeam_channel;
use std::{thread, time};

fn parallel_map<T, U, F>(input_vec: Vec<T>, num_threads: usize, f: F) -> Vec<U>
where
    F: FnOnce(T) -> U + Send + Copy + 'static,
    T: Send + 'static,
    U: Send + 'static + Default,
{
    let mut output_vec: Vec<U> = Vec::with_capacity(input_vec.len());
    let (in_sender, in_receiver) = crossbeam_channel::unbounded::<(usize, T)>();
    let (out_sender, out_receiver) = crossbeam_channel::unbounded();
    let mut threads = Vec::new();
    for _ in 0..num_threads {
        let in_receiver = in_receiver.clone();
        let out_sender = out_sender.clone();
        threads.push(thread::spawn(move || {
            while let Ok(pair) = in_receiver.recv() {
                let out = (pair.0, f(pair.1));
                out_sender.send(out).unwrap();
            }
        }))
    }
    for (i, num) in input_vec.into_iter().enumerate() {
        in_sender.send((i, num)).unwrap();
    }
    drop(in_sender);
    drop(out_sender);

    for thread in threads {
        thread.join().unwrap();
    }
    unsafe { output_vec.set_len(output_vec.capacity()); }
    while let Ok((i, ans)) = out_receiver.recv() {
        output_vec[i] = ans;
    }
    output_vec
}

fn main() {
    let v = vec![6, 7, 8, 9, 10, 1, 2, 3, 4, 5, 12, 18, 11, 5, 20];
    let squares = parallel_map(v, 10, |num| {
        println!("{} squared is {}", num, num * num);
        thread::sleep(time::Duration::from_millis(500));
        num * num
    });
    println!("squares: {:?}", squares);
}
