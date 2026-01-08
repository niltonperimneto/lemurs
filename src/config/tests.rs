use super::*;
use ratatui::style::{Color, Modifier};

#[test]
fn test_str_to_color_basic() {
    assert_eq!(str_to_color("red"), Some(Color::Red));
    assert_eq!(str_to_color("Blue"), Some(Color::Blue)); // Case insensitive
    assert_eq!(str_to_color("unknown"), None);
}

#[test]
fn test_str_to_color_hex() {
    assert_eq!(str_to_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
    assert_eq!(str_to_color("#00FF00"), Some(Color::Rgb(0, 255, 0)));
    assert_eq!(str_to_color("#0000FF"), Some(Color::Rgb(0, 0, 255)));
}

#[test]
fn test_get_modifier() {
    assert_eq!(get_modifier("Bold"), Some(Modifier::BOLD));
    assert_eq!(get_modifier("italic"), Some(Modifier::ITALIC));
    assert_eq!(get_modifier("unknown"), None);
}

#[test]
fn test_variable_iterator() {
    assert_eq!(VariableIterator::new("").next(), None);
    assert_eq!(VariableIterator::new("abcdef").next(), None);

    // Manual assertions instead of complex macro reuse
    {
        let mut iter = VariableIterator::new("$a");
        assert_eq!(iter.next().unwrap().ident, "a");
        assert_eq!(iter.next(), None);
    }
    {
        let mut iter = VariableIterator::new("$a$b");
        assert_eq!(iter.next().unwrap().ident, "a");
        assert_eq!(iter.next().unwrap().ident, "b");
        assert_eq!(iter.next(), None);
    }
}
