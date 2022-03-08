/* The following exercises were borrowed from Will Crichton's CS 242 Rust lab. */

use std::collections::HashSet;

fn main() {
    let mut v = vec![3, 1, 0, 1, 4, 4];
    dedup(&mut v);
    println!("{:?}", v);
    println!("Hi! Try running \"cargo test\" to run tests.");
}

fn add_n(v: Vec<i32>, n: i32) -> Vec<i32> {
    let mut v2 = v.clone();
    for x in v2.iter_mut() {
        *x += n;
    }
    v2
}

fn add_n_inplace(v: &mut Vec<i32>, n: i32) {
    for x in v.iter_mut() {
        *x += n;
    }
}

fn dedup(v: &mut Vec<i32>) {
    let mut seen = HashSet::new();
    let mut cnt = 0;
    for i in 0..v.len() {
        if seen.contains(&v[i - cnt]) {
            println!("Removing {} {}, cnt: {}", i - cnt, v[i - cnt], cnt);
            v.remove(i - cnt);
            cnt += 1;
        } else {
            seen.insert(v[i - cnt]);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_add_n() {
        assert_eq!(add_n(vec![1], 2), vec![3]);
    }

    #[test]
    fn test_add_n_inplace() {
        let mut v = vec![1];
        add_n_inplace(&mut v, 2);
        assert_eq!(v, vec![3]);
    }

    #[test]
    fn test_dedup() {
        let mut v = vec![3, 1, 0, 1, 4, 4];
        dedup(&mut v);
        assert_eq!(v, vec![3, 1, 0, 4]);
    }
}
