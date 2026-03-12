/// https://github.com/serde-rs/serde/issues/368#issuecomment-3171427522
///
/// Create functions with const generics to use as serde defaults. This is a
/// very hacky solution and should be replaced when this
/// [issue](https://github.com/serde-rs/serde/issues/368) is resolved.
macro_rules! ty {
    ($ty: ty, $fn_name: tt) => {
        pub const fn $fn_name<const T: $ty>() -> $ty {
            T
        }
    };
}

ty!(usize, usize);
ty!(u32, u32);
ty!(u64, u64);
ty!(bool, bool);
