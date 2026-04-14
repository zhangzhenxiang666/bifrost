pub mod request;
pub mod response;
pub mod stream;

use rand::Rng;

const ITEM_ID_CHARSET: &[u8; 62] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn create_item_id(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(prefix.len() + 1 + 16);
    id.push_str(prefix);
    id.push('_');
    for _ in 0..16 {
        id.push(ITEM_ID_CHARSET[rng.gen_range(0..ITEM_ID_CHARSET.len())] as char);
    }
    id
}
