use beam::*;

fn main() {
  println!("A");
  let foo = beam::routines::spawn(|| {
    println!("Testing my_library!");
  });
  println!("B");
//  beam::routines::wait(foo);
}
