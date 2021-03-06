extern crate bcrypt;

use anyhow::anyhow;
use bcrypt::{hash, verify, DEFAULT_COST};
use chacha20poly1305::{
    aead::{Aead, NewAead},
    XChaCha20Poly1305,
};
use rand::rngs::OsRng;
use rand::RngCore;
use std::fs;
use std::fs::{read_dir, File};
use std::io::{BufRead, BufReader, Read, Result, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::str;
use std::thread;

// function to handle all user commands after connection
fn handle_client(mut stream: TcpStream) {
    let mut buffer = vec![0; 4096];
    // keep track of currently logged in user
    let mut current_user: String = String::from("");

    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);

    loop {
        // every loop, read into buffer from TCP stream
        match stream.read(&mut buffer) {
            Ok(size) => {
                // commands sent to server by client
                let command = String::from_utf8_lossy(&buffer[0..size]);
                let words: Vec<&str> = command.trim().split_whitespace().collect();

                // if command is empty, continue to next loop to prevent out of bound errors
                if words.len() == 0 {
                    continue;
                }

                // handle different commands
                if words[0] == "upload" {
                    //get file path
                    let mut public: bool = false;
                    //let mut path = PathBuf::from("./server_publicFiles/");
                    let mut path: PathBuf = PathBuf::new();

                    //push filename to path, either public or private
                    if words[1] == "-p" {
                        path = PathBuf::from("./server_publicFiles/");
                        path.push(&words[3]);
                        public = true;
                    } else {
                        path = PathBuf::from(format!("./server_privateFiles/{}/", current_user));
                        path.push(&words[2]);
                    }

                    //create file
                    let mut file = std::fs::File::create(&path).expect("Error creating file");

                    //write data into file opened earlier
                    if public {
                        match file.write(words[4..].join(" ").as_bytes()) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("Error writing to file: {}", e);
                            }
                        }
                    } else {
                        println!("BEFORE ENCRYPTION: {}", words[3..].join(" "));
                        let encrypted = encrypt_file(words[3..].join(" "), &key, &nonce);
                        println!("AFTER ENCRYPTION: {:?}", encrypted);

                        match file.write(&encrypted) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("Error writing to file: {}", e);
                            }
                        }
                    }

                    // println!("File uploaded");
                } else if words[0] == "download" {
                    if let Err(e) = send_file(&stream, &words, &current_user, &key, &nonce) {
                        println!("The file was not able to be downloaded: {:?}", e);
                    }
                } else if words[0] == "search" {
                    if let Err(e) = search(&stream, &words, &current_user) {
                        println!("Search Unsucessful: {:?}", e);
                    }
                } else if words[0] == "login" {
                    if let Err(e) = login(&stream, &words[1], &words[2]) {
                        println!("Login Unsuccessful: {:?}", e);
                    }

                    current_user = words[1].to_string();
                } else if words[0] == "makePublic" {
                    if let Err(e) = makePublic(&stream, &words[1], &current_user) {
                        println!("File visibility Change Unsuccessful: {:?}", e);
                    }
                } else if words[0] == "makePrivate" {
                    if let Err(e) = makePrivate(&stream, &words[1], &current_user) {
                        println!("File visibility Change Unsuccessful: {:?}", e);
                    }
                } else if words[0] == "create" {
                    let mut create_result = String::from("Hello\n");
                    // check if username exists
                    let mut username_found= false;
                    // hash the password
                    let hashed_password = hash(&words[2], DEFAULT_COST).unwrap();
                    let user_input = words[1];
                    let user_info = format!("{}={}", &user_input, hashed_password);

                    // read list of existing users from users.txt
                    let f = File::open("./users/users.txt").expect("Unable to open file");
                    let f = BufReader::new(f);

                    let mut existing_users: Vec<String> = Vec::new();

                    // store the list of existing users (username=password) in a vector
                    for line in f.lines() {
                        let line = line.expect("Unable to read line");
                        existing_users.push(line);
                    }

                    // loop through the list of existing users and check if username exists
                    for user in existing_users.clone().into_iter() {
                        // split "username=password" into username and password
                        let user_info: Vec<&str> = user[..].split("=").collect();

                        // if username already exists
                        if user_info[0] == user_input {
                            username_found = true;
                        }
                    }

                    // if the username does not already exist, append the new user_info to the existing user_info
                    if !username_found {
                        existing_users.push(user_info.clone());
                        // join the list of existing users into a string separated by carriage returns
                        let joined_users = existing_users.join("\n");

                        // write new list of users to the users.txt file
                        let path = Path::new("./users/users.txt");
                        let mut file = File::create(&path).expect("Error opening file");
                        file.write_all(joined_users[..].as_bytes())
                            .expect("Unable to write data");
                        let new_directory_path = format!("./server_privateFiles/{}", &words[1]);

                        // create a new user directory under server_publicFiles
                        fs::create_dir_all(new_directory_path).unwrap();
                        create_result = String::from("User Created\n");
                    }
                    else {
                        create_result = String::from("Username already exists\n");
                    }

                    // send result to client
                    match stream.write(&create_result.as_bytes()) {
                        Ok(_) => {
                            println!("Create result sent");
                            ()
                        }
                        Err(e) => {
                            println!("Error sending result to server: {}", e);
                        }
                    }

                }
            } // ok
            Err(_) => {
                println!(
                    "An error occurred, terminating connection with {}",
                    stream.peer_addr().unwrap()
                );
                break;
            } // err
        } // match
    } // loop
}

fn main() {
    // tcp socket to communicate with client on
    let listener = TcpListener::bind("localhost:7878").unwrap();

    println!("Server listening on: {}", listener.local_addr().unwrap());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // println!("New connection: {}", stream.peer_addr().unwrap());
                // start handling the client in a new thread
                thread::spawn(move || handle_client(stream));
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }
}

// function to handle copying file from user's private storage to public
fn makePublic(mut stream: &TcpStream, filename: &str, user: &str) -> Result<()> {
    let publicPath = PathBuf::from(format!("./server_publicFiles/{}", filename));
    let privatePath = PathBuf::from(format!("./server_privateFiles/{}/{}", user, filename));

    //creates file to copy to
    let mut publicFile = std::fs::File::create(&publicPath).expect("Error creating file");

    match fs::copy(privatePath, publicPath){
        Ok(u64) => (()),
        Err(e) => {
            println!("File Not Found");
            fs::remove_file(PathBuf::from(format!("./server_publicFiles/{}", filename)));
        },
    }

    Ok(())
}

// function to handle copying file from public to user's private storage
fn makePrivate(mut stream: &TcpStream, filename: &str, user: &str) -> Result<()> {
    let publicPath = PathBuf::from(format!("./server_publicFiles/{}", filename));
    let privatePath = PathBuf::from(format!("./server_privateFiles/{}/{}", user, filename));

    //creates file to copy to
    let mut publicFile = std::fs::File::create(&privatePath).expect("Error creating file");

    match fs::copy(publicPath, privatePath){
        Ok(u64) => (()),
        Err(e) => {
            println!("File Not Found");
            fs::remove_file(PathBuf::from(format!("./server_privateFiles/{}/{}", user, filename)));
        },
    }

    Ok(())
}

// function to perform login
fn login(mut stream: &TcpStream, givenUsername: &str, givenPassword: &str) -> Result<()> {
    // println!(
    //     "Login - Username: {}, Password: {}",
    //     givenUsername, givenPassword
    // );
    let mut loginResult = String::from("Hello\n");
    // check if username exists
    let mut usernameFound = false;

    //read from existing users file
    let f = File::open("./users/users.txt").expect("Unable to open file");
    let f = BufReader::new(f);

    let mut existing_users: Vec<String> = Vec::new();

    //store users in vec
    for line in f.lines() {
        let line = line.expect("Unable to read line");
        existing_users.push(line);
    }

    // check if user is found in list of existing users
    for username in existing_users.clone().into_iter() {
        let user_info: Vec<&str> = username[..].split("=").collect();

        if user_info[0] == givenUsername {
            usernameFound = true;
            println!("Username matched");

            // if it exists, check the text file and compare the hashpassword
            match verify(givenPassword, user_info[1]) {
                Ok(boo) => {
                    if boo {
                        println!("Login Successful");
                        loginResult = String::from("Login Successful\n");
                    } else {
                        println!("Password Incorrect");
                        loginResult = String::from("Password Incorrect\n");
                    }
                }
                Err(e) => println!("{}", e),
            }
        }
    }

    // if username is not found, send error message to user
    if !usernameFound {
        println!("Username not found");
        loginResult = String::from("Username not found\n");
    }

    // send result to client
    match stream.write(&loginResult.as_bytes()) {
        Ok(_) => {
            println!("Login result sent");
            ()
        }
        Err(e) => {
            println!("Error sending result to server: {}", e);
        }
    }

    Ok(())
}

// function to handle search for server
fn search(mut stream: &TcpStream, command: &Vec<&str>, user: &str) -> Result<()> {
    // if searching in public folder
    let public_option = command.contains(&"-p");
    // if searching only extensions
    let ext_option = command.contains(&"-x");
    // vector of file names matching given name
    let mut files_in_dir: Vec<String> = Vec::new();
    // String which will be sent as server response
    let mut data = String::new();
    let path: PathBuf;
    // set path based on if searching public files or not
    if public_option {
        path = PathBuf::from("./server_publicFiles/");
    } else {
        path = PathBuf::from(format!("./server_privateFiles/{}/", user));
    }
    // get list of entries in a directory
    match read_dir(Path::new(&path)) {
        Ok(dir_files) => {
            // for each entry check if it is not a directory
            for entry in dir_files {
                let entry = entry?;
                let file_path = entry.path();
                if !file_path.is_dir() {
                    if let Ok(name) = entry.file_name().into_string() {
                        // then check if entry matches based on extension or name based on options given
                        if ext_option {
                            if let Some(ext) = file_path.extension() {
                                if let Some(last_elem) = command.last() {
                                    if ext.to_str() == Some(last_elem) {
                                        // add matching results to vector
                                        files_in_dir.push(name.clone());
                                    }
                                }
                            }
                        } else {
                            if let Some(last_elem) = command.last() {
                                if name.contains(last_elem) {
                                    files_in_dir.push(name.clone());
                                }
                            }
                        }
                    }
                }
            }
            for file_name in files_in_dir {
                data.push_str(&file_name);
                data.push_str(" ");
            }
        }
        Err(e) => {
            println!("Error searching for file: {}", e);
        }
    }
    data.push_str("\n");
    // send result vector to client
    match stream.write(&data.as_bytes().to_vec()) {
        Ok(_) => {
            println!("Search results sent");
            ()
        }
        Err(e) => {
            println!("Error sending search request to client: {}", e);
        }
    }
    Ok(())
}

// function for download command
fn send_file(
    mut stream: &TcpStream,
    command: &Vec<&str>,
    user: &str,
    key: &[u8; 32],
    nonce: &[u8; 24],
) -> Result<()> {
    let mut path: PathBuf;
    // downloading either from public or user directories
    if command[1] == "-p" {
        path = PathBuf::from("./server_publicFiles/");
        path.push(command[2]);
    } else {
        path = PathBuf::from("./server_privateFiles/");
        path.push(format!("{}/", user));
        path.push(command[1]);
    }

    let file: File;
    match File::open(Path::new(&path)) {
        Ok(open_result) => {
            file = open_result;
        }
        Err(e) => {
            println!("Error opening file: {}", e);
            stream.write("\n".as_bytes()).unwrap();
            return Err(e);
        }
    }

    let mut file_size;
    let mut decrypted: Vec<u8> = Vec::new();

    if command[1] == "-p" {
        match file.metadata() {
            Ok(meta) => {
                file_size = meta.len();
            }
            Err(e) => {
                println!("Error parsing file size: {}", e);
                return Err(e);
            }
        }
    } else {
        let file_data = fs::read(&path)?;
        decrypted = decrypt_file(&file_data, key, nonce);
        file_size = decrypted.len() as u64;
    }

    // file length string ending with \n so server knows when to stop reading
    let mut file_length = file_size.to_string();
    file_length.push_str("\n");

    // send file size
    match stream.write(&file_length.as_bytes()) {
        Ok(_) => {
            println!("File size sent");
            ()
        }
        Err(e) => {
            println!("Error sending file size to server: {}", e);
        }
    }

    // read data from file to send to client
    let mut buffer = Vec::new();

    match File::open(&path) {
        Ok(mut file) => {
            match file.read_to_end(&mut buffer) {
                Ok(_) => (),
                Err(e) => {
                    println!("Error reading file to copy data: {}", e);
                }
            };
        }
        Err(e) => {
            println!("Error opening file to copy data: {}", e);
        }
    };

    // decrypt the file if downloaded from the private directory
    if command[1] != "-p" {
        match stream.write(&decrypted) {
            Ok(_) => {
                println!("Decrypted file data sent");
                ()
            }
            Err(e) => {
                println!("Error sending file data to server: {}", e);
            }
        }
    } else {
        // send file data without decrypting
        match stream.write(&buffer) {
            Ok(_) => {
                println!("File data sent");
                ()
            }
            Err(e) => {
                println!("Error sending file data to server: {}", e);
            }
        }
    }

    println!("File sent successfully!");
    Ok(())
}

// function encrypt file upon upload
fn encrypt_file(file_data: String, key: &[u8; 32], nonce: &[u8; 24]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());

    let encrypted_file = cipher
        .encrypt(nonce.into(), file_data.as_ref())
        .map_err(|err| anyhow!("Encrypting file: {}", err))
        .unwrap();

    encrypted_file
}

// function to decrypt file upon download
fn decrypt_file(file_data: &Vec<u8>, key: &[u8; 32], nonce: &[u8; 24]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());

    let decrypted_file = cipher
        .decrypt(nonce.into(), file_data.as_ref())
        .map_err(|err| anyhow!("Decrypting file: {}", err))
        .unwrap();

    decrypted_file
}
