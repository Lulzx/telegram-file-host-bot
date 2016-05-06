extern crate telegram_bot;
extern crate hyper;
extern crate rand;

use rand::Rng;
use std::thread::spawn;
use std::env::var;
use std::io;
use std::fs::File;
use std::path::{Path,PathBuf};
use telegram_bot::{Api, MessageType, ListeningMethod, ListeningAction};
use telegram_bot::types::{User,Integer};
use hyper::Url;
use hyper::method::Method;
use hyper::client::{Request};

const ENV_TOKEN: &'static str = "BOT_TOKEN";
const ENV_DOWNLOAD_DIR: &'static str = "DOWNLOAD_DIR";
const ENV_BASE_URL: &'static str = "BASE_URL";

struct TGFile {
    file_id: String,
    file_size: Integer
}

impl TGFile {
    fn from_message(msg: MessageType) -> Option<TGFile> {
        if let Some((file_id, Some(file_size))) = match msg.clone() {
            MessageType::Photo(photos) => {
                let largest_photo = photos.last().unwrap();
                Some((largest_photo.file_id.clone(), largest_photo.file_size))
            },
            MessageType::Sticker(sticker) => Some((sticker.file_id, sticker.file_size)),
            MessageType::Document(document) => Some((document.file_id, document.file_size)),
            MessageType::Audio(audio) => Some((audio.file_id, audio.file_size)),
            MessageType::Video(video) => Some((video.file_id, video.file_size)),
            MessageType::Voice(voice) => Some((voice.file_id, voice.file_size)),
            _ => None
        } {
            Some(TGFile{ file_id: file_id, file_size: file_size})
        }
        else {
            None
        }
    }
}

fn generate_name() -> String {
    let mut rng = rand::thread_rng();
    rng.gen_ascii_chars().take(6).collect()
}

fn replace_filename(filename: &str, name: &str) -> String {
    match filename.split('.').last() {
        Some(ext) => format!("{}.{}", name, ext),
        None => name.into()
    }
}

fn download_to_file(url: &Url, destination: &Path) -> io::Result<()>{
    // Create a request to download the file
    let req = Request::new(Method::Get, url.clone()).unwrap();
    let mut resp = req.start().unwrap().send().unwrap();

    // Open file and copy downloaded data
    let mut file = try!(File::create(destination));
    try!(std::io::copy(&mut resp, &mut file));

    Ok(())
}

fn download_file(url: &Url, destination: &Path, baseurl: &Url) -> io::Result<Url> {
    // Grab the last portion of the url
    let filename = url.path().unwrap().last().unwrap();

    // Create path by combining filename from url with download dir
    let hash = generate_name();
    let filename = replace_filename(&filename, &hash);
    let mut path = destination.to_path_buf();
    path.set_file_name(&filename);

    try!(download_to_file(url, &path));

    // Create the return url that maps to this filename
    let returl = baseurl.join(&filename).unwrap();
    Ok(returl)
}

fn download_file_user(url: &Url, user: &User, base_download_dir: &Path, base_url: &Url) -> io::Result<Url> {
    // Create the final download directory by combining the base
    // directory with the username, and ensure it exists.
    let mut download_dir_user = base_download_dir.to_path_buf();
    let user_path = user_path(&user);
    download_dir_user.push(user_path.clone());
    ensure_dir(&download_dir_user);

    // Create the final URL by combining the base URL and the
    // username.
    let base_url_user = base_url.join(&user_path).unwrap();

    download_file(&url, &download_dir_user, &base_url_user)
}

fn ensure_dir(path: &Path) {
    let _ = std::fs::create_dir(&path);
}

fn user_path(user: &User) -> String {
    match user.username {
        Some(ref name) => name.clone(),
        None => "anonymous".into()
    }
}

fn main() {
    let api = Api::from_env(ENV_TOKEN)
        .expect(&format!("Must set environment variable {}.", ENV_TOKEN));

    let download_dir = var(ENV_DOWNLOAD_DIR)
        .map(|s| PathBuf::from(s))
        .expect(&format!("Must set {} to a valid path", ENV_DOWNLOAD_DIR));

    let base_url = var(ENV_BASE_URL)
        .map(|s| Url::parse(&s))
        .expect(&format!("Must set {} to a valid url", ENV_BASE_URL)).unwrap();

    println!("getMe: {:?}", api.get_me());

    let mut listener = api.listener(ListeningMethod::LongPoll(None));

    ensure_dir(&download_dir);

    let tg_listener = spawn(move || {
        listener.listen(|u| {
            if let Some(m) = u.message {
                let user = m.from.clone();


                // Attempt to download the file to the user's subdirectory
                if let Some(tgfile) = TGFile::from_message(m.msg.clone()) {
                    let file = api.get_file(&tgfile.file_id).unwrap();
                    if let Some(path) = file.file_path {
                        // Get the file's URL on Telegram's server
                        let tg_url = Url::parse(&api.get_file_url(&path)).unwrap();

                        // Download the final file and create the URL for the user
                        let client_url = download_file_user(&tg_url, &user, &download_dir, &base_url).unwrap();
                        println!("[INFO] {} direct upload {} ({})", user_path(&user), client_url, tgfile.file_size);
                        let _ = api.send_message(
                            m.chat.id(),
                            format!("{}", client_url),
                            None, None, None, None).unwrap();
                    }
                }

                // Handle URLs sent to us for rehosting.
                 if let MessageType::Text(txt) = m.msg {
                     if let Ok(url) = Url::parse(&txt) {
                        // Download the final file and create the URL for the user
                         let client_url = download_file_user(&url, &user, &download_dir, &base_url).unwrap();
                         println!("[INFO] {} rehost {} from {}", user_path(&user), client_url, url);
                         let _ = api.send_message(
                             m.chat.id(),
                             format!("{}", client_url),
                             None, None, None, None).unwrap();
                     }
                 }
            }
            Ok(ListeningAction::Continue)
        }).unwrap();
    });

    println!("Handling telegram requests!");

    tg_listener.join().unwrap();
}
