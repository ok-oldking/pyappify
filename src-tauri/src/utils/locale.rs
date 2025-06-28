pub fn get_locale() -> String {
    sys_locale::get_locale().map_or("en_US".to_string(), |locale| locale.replace('-', "_"))
}