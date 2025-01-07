use bitflags::*;

bitflags! {
  #[derive(Debug)]
  pub struct Permissions: u8 {
    const READ = 0b0001;
    const MOVE = 0b0010;
    const ADMINISTER = 0b0100;
  }
}
