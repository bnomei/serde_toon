use std::cell::RefCell;

use crate::arena::ArenaParts;

#[derive(Default)]
struct ArenaPool {
    parts: ArenaParts,
}

impl ArenaPool {
    fn take(&mut self) -> ArenaParts {
        std::mem::take(&mut self.parts)
    }

    fn put(&mut self, parts: ArenaParts) {
        self.parts = parts;
    }
}

thread_local! {
    static ARENA_POOL: RefCell<ArenaPool> = RefCell::new(ArenaPool::default());
}

pub fn take_arena_parts() -> ArenaParts {
    ARENA_POOL.with(|pool| pool.borrow_mut().take())
}

pub fn put_arena_parts(parts: ArenaParts) {
    ARENA_POOL.with(|pool| pool.borrow_mut().put(parts));
}
