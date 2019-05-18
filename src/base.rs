/// Represents an identifier used in Kademlia.
///
/// These identifiers are used to represent two similar things:
///
/// * key hashes for locating values
///
/// * unique identifiers for nodes participating in the DHT
///
/// Both of these share the same `distance metric`, which allows
/// us to compare two keys and tell how "far apart" they are. This is a
/// central part of how the DHT works, because it enables us to more
/// efficiently query for keys.
///
/// As a consequence of these different use cases, this type has behavior,
/// e.g. the distance metric we mentioned before, but has no semantic
/// meaning by itself, since it can be used to mean one of these 2 things
/// depending on the situation.
#[derive(Clone, Copy, Debug)]
pub struct BitKey(pub u128);

impl BitKey {
    /// Calculate the distance between two keys.
    ///
    /// The distance is based on the "xor-metric", which is just the
    /// xor of the underlying numbers for each key.
    ///
    /// The most important aspect of the distance function is that it
    /// satisfies the definition of a
    /// [metric](https://en.wikipedia.org/wiki/Metric_(mathematics)#Definition).
    ///
    /// # Properties
    ///
    /// * non-negativity
    ///
    /// `x.distance(y) >= 0`
    ///
    /// * identity of indiscernables
    ///
    /// `x.distance(y) = 0 <=> x = y`
    ///
    /// * symmetry
    ///
    /// `x.distance(y) = y.distance(x)`
    ///
    /// * triangle inequality
    ///
    /// `x.distance(z) <= x.distance(y) + y.distance(z)`
    pub fn distance(self, other: BitKey) -> u128 {
        self.0 ^ other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        let a = BitKey(1);
        let b = BitKey(2);
        assert_eq!(3, a.distance(b));
        assert_eq!(3, b.distance(a));
        assert_eq!(0, a.distance(a));
        let z = BitKey(0);
        assert_eq!(a.0, z.distance(a));
        assert_eq!(b.0, z.distance(b));
    }
}
