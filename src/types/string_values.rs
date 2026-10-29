// Reversible enum string mappings shared by persisted task and memory types.
// Defines forward and reverse methods from one exhaustive list.
// Deps: macro_rules only; unknown values remain None.

macro_rules! string_values {
    ($ty:ty, $label:ident(&$this:ident), $vis:vis $parse:ident { $($variant:ident => $value:literal),+ $(,
        )? }) => {
        $crate::types::string_values::string_values!(@impl $ty, $label, [&$this], $this,
            $vis $parse { $($variant => $value),+ });
    };
    ($ty:ty, $label:ident($this:ident), $vis:vis $parse:ident { $($variant:ident => $value:literal),+ $(,
        )? }) => {
        $crate::types::string_values::string_values!(@impl $ty, $label, [$this], $this,
            $vis $parse { $($variant => $value),+ });
    };
    (@impl $ty:ty, $label:ident, [$($receiver:tt)+], $this:ident,
        $vis:vis $parse:ident { $($variant:ident => $value:literal),+ }) => {
        impl $ty {
            pub fn $label($($receiver)+) -> &'static str {
                match $this { $(Self::$variant => $value),+ }
            }

            $vis fn $parse(value: &str) -> Option<Self> {
                match value { $($value => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

pub(super) use string_values;
