//!
//! Path Info
//!

#[repr(transparent)]
pub struct PathInfo(str);

pub struct PathInfoIter<'a> {
    s: &'a str,
}

impl<'a> PathInfo {
    pub fn new(s: &'a str) -> &'a Self {
        unsafe { &*(s as *const str as *const Self) }
    }

    pub fn iter(&self) -> PathInfoIter<'_> {
        PathInfoIter {
            s: unsafe { &*(self as *const Self as *const str) },
        }
    }

    pub fn as_str(&self) -> &str {
        unsafe { &*(self as *const Self as *const str) }
    }

    pub fn is_absolute_path(&self) -> bool {
        unsafe { &*(self as *const Self as *const str) }.starts_with('/')
    }
}

impl<'a> Iterator for PathInfoIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.s.len() == 0 {
            return None;
        }
        let mut index = 0;
        for e in self.s.chars() {
            if e == '/' {
                let (c, n) = self.s.split_at(index);
                self.s = n.split_at(1).1;
                return Some(c);
            }
            index += 1;
        }
        let (c, n) = self.s.split_at(index);
        self.s = n;
        return Some(c);
    }
}
