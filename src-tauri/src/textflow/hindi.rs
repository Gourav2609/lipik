//! Common Hindi words written in Roman letters, shared by the speech-engine
//! choice (`apple_speech::pick`) and the clean-up checks.

pub const WORDS: &[&str] = &[
    "aap", "aapka", "aapki", "abhi", "acha", "accha", "agar", "apka", "apna", "apni", "aur",
    "baad", "bata", "batao", "bhai", "bhi", "bolo", "chahiye", "dekho", "dena", "dedo",
    "gaya", "gaye", "gayi", "haan", "hai", "hain", "ho", "hoga", "hoge", "hogi", "hona", "hoon",
    "hua", "hui", "jaldi", "ka", "kab", "kaise", "kal", "kar", "karke", "karna", "karo", "kaun",
    "ke", "ki", "kitna", "ko", "kuch", "kya", "kyun", "lekin", "mein", "mera", "mere", "meri",
    "mujhe", "nahi", "nahin", "nhi", "phir", "raha", "rahe", "rahi", "sab", "se", "tak", "tha",
    "theek", "thi", "thik", "toh", "tum", "unko", "wala", "wale", "wali", "yaar", "yeh", "woh",
];

/// Words of `WORDS` that are also English words or halves of English phrases
/// ("a mere formality", "per se").
pub const ALSO_ENGLISH: &[&str] = &["agar", "mere", "se", "wale"];
