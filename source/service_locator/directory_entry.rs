use std::cmp::*;
use std::u32;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DirectoryCategory {
  None,
  Account,
  Directory,
}

impl Default for DirectoryCategory {
  fn default() -> Self {
    DirectoryCategory::None
  }
}

#[derive(Clone, Debug, Eq, PartialOrd)]
pub struct DirectoryEntry {
  pub category: DirectoryCategory,
  pub id: u32,
  pub name: String,
}

impl DirectoryEntry {
  pub fn root_directory() -> Self {
    DirectoryEntry::new(DirectoryCategory::Account, 1, String::from("root"))
  }

  pub fn star_directory() -> Self {
    DirectoryEntry::new(DirectoryCategory::Directory, 0, String::from("0"))
  }

  pub fn new(category: DirectoryCategory, id: u32, name: String) -> Self {
    DirectoryEntry {
      category: category,
      id: id,
      name: name,
    }
  }

  pub fn new_account(id: u32, name: String) -> Self {
    DirectoryEntry::new(DirectoryCategory::Account, id, name)
  }

  pub fn new_directory(id: u32, name: String) -> Self {
    DirectoryEntry::new(DirectoryCategory::Directory, id, name)
  }
}

impl Default for DirectoryEntry {
  fn default() -> Self {
    DirectoryEntry::new(
      DirectoryCategory::default(),
      u32::MAX,
      String::default(),
    )
  }
}

impl Ord for DirectoryEntry {
  fn cmp(&self, other: &Self) -> Ordering {
    self.id.cmp(&other.id)
  }
}

impl PartialEq for DirectoryEntry {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
  }
}
