use std::io::Result as IoResult;

pub trait Repository {
    fn save(&self, item: Item);
    fn find(&self, id: u32) -> Item;
}

pub struct Item {
    pub name: String,
    pub id: u32,
}

pub struct Service {
    repo: Box<dyn Repository>,
}

impl Service {
    pub fn new(repo: Box<dyn Repository>) -> Self {
        Service { repo }
    }

    pub fn process(&self) {
        let item = self.repo.find(1);
        self.repo.save(item);
    }
}

pub enum Status {
    Active,
    Inactive { reason: String },
}

pub type AppResult<T> = std::result::Result<T, String>;
