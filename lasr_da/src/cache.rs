use std::hash::Hash;

pub trait LruCache: IntoIterator {
    type Value: Hash;

    fn cache(&mut self, item: &Self::Value); 
    //TODO: Add a method to clean the cache at a configurable duration
    //fn clean(&mut self, n: usize);
    fn get(&self, key: &Self::Value) -> Option<&Self::Value>;
}
