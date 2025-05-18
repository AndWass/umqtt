use core::marker::PhantomData;

#[derive(Clone)]
struct Deref<'a>(core::slice::Iter<'a, &'a str>);

impl<'a> Iterator for Deref<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

/// An iterator that iterates over one or multiple strings and can create a topic string from this
///
/// A topic iterator that iterates over `["hello", "world"] would represent the topic
/// `hello/world`.
///
/// This is mainly used within [`PublishTopicIterator`].
pub struct TopicIterator<'a, I: Iterator + Sized> {
    iter: core::iter::Peekable<I>,
    first: bool,
    prev_was_separator: bool,
    _marker: PhantomData<&'a ()>,
}

impl<'a, I> Clone for TopicIterator<'a, I>
where
    I: Iterator<Item=&'a str> + Sized + Clone
{
    fn clone(&self) -> Self {
        Self {
            iter: self.iter.clone(),
            first: self.first,
            prev_was_separator: self.prev_was_separator,
            _marker: PhantomData,
        }
    }
}

impl<I: Iterator + Sized> TopicIterator<'_, I> {
    pub fn new(iter: I) -> Self {
        Self {
            iter: iter.peekable(),
            first: true,
            prev_was_separator: false,
            _marker: PhantomData,
        }
    }
}

impl<'a> From<&'a str> for TopicIterator<'a, core::iter::Once<&'a str>> {
    fn from(iter: &'a str) -> Self {
        Self::new(core::iter::once(iter))
    }
}

impl<'a: 'b, 'b> From<&'a[&'b str]> for TopicIterator<'a, Deref<'b>> {
    fn from(iter: &'a[&'b str]) -> Self {
        Self::new(Deref(iter.iter()))
    }
}

impl<'b, const N: usize> From<[&'b str; N]> for TopicIterator<'b, core::array::IntoIter<&'b str, N>> {
    fn from(value: [&'b str; N]) -> Self {
        Self::new(value.into_iter())
    }
}

impl<'a, I: Iterator<Item=&'a str> + Sized> Iterator for TopicIterator<'a, I> {
    type Item=I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        if self.first {
            self.first = false;
            self.iter.next()
        }
        else if !self.prev_was_separator {
            match self.iter.peek() {
                Some(_x) => {
                    self.prev_was_separator = true;
                    Some("/")
                },
                None => {
                    None
                }
            }
        }
        else {
            self.prev_was_separator = false;
            self.iter.next()
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::ToOwned;
    use super::*;

    #[test]
    fn single_string() {
        let mut iter: TopicIterator<_> = "hello".into();
        assert_eq!(iter.next(), Some("hello"));
        assert_eq!(iter.next(), None);

        let mut iter: TopicIterator<_> = ["hello"].into();
        assert_eq!(iter.next(), Some("hello"));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn array() {
        let mut iter: TopicIterator<_> = ["hello", "world"].into();
        assert_eq!(iter.next(), Some("hello"));
        assert_eq!(iter.next(), Some("/"));
        assert_eq!(iter.next(), Some("world"));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn slice() {
        let s = "world".to_owned();
        let slice = ["hello", s.as_str()];
        let mut iter: TopicIterator<_> = slice.as_slice().into();
        assert_eq!(iter.next(), Some("hello"));
        assert_eq!(iter.next(), Some("/"));
        assert_eq!(iter.next(), Some("world"));
        assert_eq!(iter.next(), None);
    }
}