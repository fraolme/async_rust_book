use std::ptr;

struct SelfReferential {
    data: String,
    self_pointer: *const String,
}

impl SelfReferential {
    fn new(data: String) -> SelfReferential {
        let mut sr = SelfReferential {
            data,
            self_pointer: ptr::null(),
        };
        sr.self_pointer = &sr.data as *const String;
        sr
    }

    fn print(&self) {
        unsafe {
            println!("{}", *self.self_pointer);
        }
    }
}

fn main() {
    let first = SelfReferential::new("first".to_string());
    let moved_first = first; //Moved the struct
                             // this will cause an error likely a segmentation fault
                             // since the location of the struct in memory is moved
                             // that is why we need pinning with futures to make it
                             // stay in fixed memory address
    moved_first.print();
}
