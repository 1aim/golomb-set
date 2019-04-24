//! Golomb-coded Sets

#![deny(missing_docs)]

#[macro_use]
extern crate failure_derive;

use {
    bitvec::{BitVec, Bits},
    byteorder::ByteOrder,
    digest::Digest,
    failure::Error,
    num_integer::div_rem,
    std::marker::PhantomData,
};

#[derive(Debug, Fail)]
enum GcsError {
    #[fail(display = "The limit for the number of elements has been reached")]
    LimitReached,
}

/// Builder for a GCS
#[derive(Clone, Debug)]
pub struct GcsBuilder<D: Digest> {
    n: u64,
    p: u8,
    values: Vec<u64>,
    digest: PhantomData<D>,
}

impl<D: Digest> GcsBuilder<D> {
    /// Creates a new GcsBuilder from n and p, where n is the number of items
    /// to be stored in the set and 1/2^p is the probability of a false positive
    pub fn new(n: u64, p: u8) -> Self {
        GcsBuilder {
            n,
            p,
            values: Vec::new(),
            digest: PhantomData,
        }
    }

    /// Adds an entry to the set, and returns an error if more than N items are added
    pub fn insert(&mut self, input: &[u8]) -> Result<(), Error> {
        if (self.values.len() as u64) < self.n {
            self.values.push(digest_value::<D>(self.n, self.p, input));
            Ok(())
        } else {
            Err(GcsError::LimitReached.into())
        }
    }

    /// Adds an entry to the set, does not error if more than N items are added
    pub fn insert_unchecked(&mut self, input: &[u8]) {
        self.values.push(digest_value::<D>(self.n, self.p, input));
    }

    /// Consumes the builder and creates the encoded set
    pub fn build(mut self) -> Gcs<D> {
        let mut out = BitVec::new();

        // Sort then calculate differences
        self.values.sort();
        for i in (1..self.values.len()).rev() {
            self.values[i] -= self.values[i - 1];
        }

        // Apply golomb encoding
        let mut bits = BitVec::<bitvec::BigEndian>::new();
        for val in self.values {
            bits.append(&mut golomb_encode(val, self.p))
        }
        out.append(&mut bits);

        Gcs::<D>::new(self.n, self.p, out)
    }
}

/// A Golomb-coded Set
pub struct Gcs<D: Digest> {
    n: u64,
    p: u8,
    bits: BitVec,
    digest: PhantomData<D>,
}

impl<D: Digest> Gcs<D> {
    /// Create a GCS from n, p and a BitVec of the Golomb-Rice encoded values,
    /// where n is the number of items the GCS was defined with and 1/2^p is
    /// the probability of a false positive
    pub fn new(n: u64, p: u8, bits: BitVec) -> Self {
        Gcs {
            n,
            p,
            bits,
            digest: PhantomData,
        }
    }

    /// Returns whether or not an input is contained in the set. If false the
    /// input is definitely not present, if true the input is probably present
    pub fn contains(&self, input: &[u8]) -> bool {
        let mut values = golomb_decode(self.bits.clone().iter().peekable(), self.p);

        for i in 1..values.len() {
            values[i] += values[i - 1];
        }

        values.contains(&digest_value::<D>(self.n, self.p, input))
    }

    /// Get the raw data bytes from a GCS
    pub fn as_bits(&self) -> &BitVec {
        &self.bits
    }

    /// Get the raw values encoded in the BitVec
    pub fn values(&self) -> Vec<u64> {
        golomb_decode(self.bits.clone().iter().peekable(), self.p)
    }
}

/// Perform Golomb-Rice encoding of n, with modulus 2^p
///
/// # Panics
///
/// Panics if `p == 0`.
fn golomb_encode(n: u64, p: u8) -> BitVec {
    if p == 0 {
        panic!("p cannot be 0");
    }
    let (quo, rem) = div_rem(n, 2u64.pow(u32::from(p)));

    let mut out = BitVec::new();

    // Unary encoding of quotient
    for _ in 0..quo {
        out.push(true);
    }
    out.push(false);

    // Binary encoding of remainder in p bits
    // remove vec and change to big end?
    for i in (0..p).rev() {
        out.push(rem.get::<bitvec::LittleEndian>(i.into()));
    }

    out
}

/// Perform Golomb-Rice decoding of n, with modulus 2^p
fn golomb_decode<I>(iter: I, p: u8) -> Vec<u64>
where
    I: Iterator<Item = bool>,
{
    let mut out = Vec::<u64>::new();
    let mut iter = iter.peekable();

    while let Some(_) = iter.peek() {
        // parse unary encoded quotient
        let mut quo = 0u64;
        while iter.next().unwrap() {
            quo += 1;
        }

        // parse binary encoded remainder
        let mut rem = 0u64;
        for _ in 0..p {
            if iter.next().unwrap() {
                rem += 1;
            }
            rem <<= 1;
        }
        rem >>= 1;

        // push quo * p + rem
        out.push(quo * 2u64.pow(u32::from(p)) + rem);
    }

    out
}

fn digest_value<D: Digest>(n: u64, p: u8, input: &[u8]) -> u64 {
    let val = if D::output_size() < 8 {
        let mut buf = [0u8; 8];
        let digest = D::digest(input);
        for i in 0..D::output_size() {
            buf[i + D::output_size()] = digest[i];
        }

        byteorder::BigEndian::read_u64(&buf)
    } else {
        byteorder::BigEndian::read_u64(&D::digest(input)[..8])
    };

    val % (n * 2u64.pow(u32::from(p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // Ranges need to be extended after improving performance
        #[test]
        fn golomb_single(n in 0u64..100000u64, p in 2u8..16) {
            assert_eq!(n, golomb_decode(golomb_encode(n, p).iter().peekable(), p)[0]);
        }
    }
}
