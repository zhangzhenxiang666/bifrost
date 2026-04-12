//! OneOrMany deserializer for handling single value or array

use serde::Deserialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use std::fmt;
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T> OneOrMany<T> {
    pub fn as_vec(&self) -> &Vec<T> {
        &self.0
    }
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        OneOrMany(Vec::new())
    }
}

struct OneOrManyVisitor<T> {
    marker: PhantomData<T>,
}

impl<T> OneOrManyVisitor<T> {
    fn new() -> Self {
        OneOrManyVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, T> Visitor<'de> for OneOrManyVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a single value or a list of values")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let single: T = Deserialize::deserialize(de::value::StrDeserializer::new(value))?;
        Ok(vec![single])
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(element) = seq.next_element()? {
            vec.push(element);
        }
        Ok(vec)
    }
}

impl<'de, T> Deserialize<'de> for OneOrMany<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let visitor = OneOrManyVisitor::new();
        deserializer.deserialize_any(visitor).map(OneOrMany)
    }
}

pub fn deserialize_one_or_many<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    OneOrMany::<T>::deserialize(deserializer).map(|v| v.into_vec())
}
