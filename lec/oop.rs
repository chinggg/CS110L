struct LinkedList {
  head: Option<Box<Node>>,
  size: usize,
}

struct Node {
  value: i32,
  next: Option<Box<Node>>
}

impl Node {
  pub fn new(value: i32, next: Option<Box<Node>>) -> Node {
    Node {value: value, next: next}
  }
}

impl LinkedList {
  pub fn new() -> LinkedList {
    LinkedList {head: None, size: 0}
  }

  pub fn get_size(&self) -> usize {
    self.size
  }

  pub fn is_empty(&self) -> bool {
    self.get_size() == 0
  }

  pub fn push(&mut self, value: i32) {
    let new_node = Box::new(Node::new(value, self.head.take()));
    self.head = Some(new_node);
    self.size += 1;
  }
  
  pub fn pop(&mut self) -> Option<i32> {
    let node = self.head.take()?;
    self.head = node.next;
    self.size -= 1;
    Some(node.value)
  }

  pub fn display(&self) {
    let mut current = &self.head;
    while let Some(node) = current {
      print!("{} ", node.value);
      current = &node.next;
    }
    println!();
  }
}

fn main() {
  let x: Box<u32> = Box::new(42);
  let head: Node = Node::new(1, None);
  let mut list = LinkedList::new();
  assert_eq!(list.get_size(), 0);
  for i in 0..10 {
    list.push(i);
  }
  println!("{}", list.is_empty());
  list.display();
  while !list.is_empty() {
    print!("{}", list.pop().unwrap());
  }
  println!();
  let mut x = Some(5);
  let x_ref = &mut x;
  println!("{:?}", x_ref.take());
  println!("{:?}", x);
}