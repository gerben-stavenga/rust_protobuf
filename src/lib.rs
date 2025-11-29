pub mod repeated_field;
pub mod wire;
pub mod decoding;
pub mod encoding;
pub mod base;

pub struct LocalCapture<'a, T> {
    value: std::mem::ManuallyDrop<T>,
    origin: &'a mut T,
}

impl<'a, T> LocalCapture<'a, T> {
    pub fn new(origin: &'a mut T) -> Self {
        Self { value: std::mem::ManuallyDrop::new(unsafe { std::ptr::read(origin) }), origin }
    }
}

impl<'a, T> std::ops::Deref for LocalCapture<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T> std::ops::DerefMut for LocalCapture<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T> Drop for LocalCapture<'a, T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::write(self.origin, std::mem::ManuallyDrop::take(&mut self.value));
        }
    }
}


