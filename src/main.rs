#[macro_use] extern crate serde_derive;
#[macro_use] extern crate failure;
extern crate clap;
extern crate colored;
extern crate ws;
extern crate futures;
extern crate tokio;
extern crate secp256k1;
extern crate rand;
extern crate sha2;
extern crate digest;
extern crate uuid;

extern crate grin_wallet;
extern crate grin_keychain;
extern crate grin_util;
extern crate grin_core;
extern crate grin_store;

use std::sync::{Arc, Mutex};
use clap::ArgMatches;
use colored::*;

use grin_core::{core};

#[macro_use] mod common;
mod grinbox;
mod wallet;
mod storage;
mod contacts;
mod cli;

use common::config::Wallet713Config;
use common::{Wallet713Error, Result};
use common::crypto::*;
use common::types::Contact;
use wallet::Wallet;
use cli::Parser;

use contacts::AddressBook;

fn do_config(args: &ArgMatches, silent: bool) -> Result<Wallet713Config> {
	let mut config;
	let mut any_matches = false;
    let exists = Wallet713Config::exists();
	if exists {
		config = Wallet713Config::from_file()?;
	} else {
		config = Wallet713Config::default()?;
	}

    if let Some(data_path) = args.value_of("data-path") {
        config.wallet713_data_path = data_path.to_string();
        any_matches = true;
    }

	if let Some(uri) = args.value_of("uri") {
		config.grinbox_uri = uri.to_string();
		any_matches = true;
	}
	
    if let Some(account) = args.value_of("private-key") {
        config.grinbox_private_key = account.to_string();
        any_matches = true;
    }

    if let Some(node_uri) = args.value_of("node-uri") {
        config.grin_node_uri = node_uri.to_string();
        any_matches = true;
    }

    if let Some(node_secret) = args.value_of("node-secret") {
        config.grin_node_secret = Some(node_secret.to_string());
        any_matches = true;
    }

    if !exists || args.is_present("generate-keys") {
        let (pr, _) = generate_keypair();
        config.grinbox_private_key = pr.to_string();
        any_matches = exists;
    }

	config.to_file()?;

    if !any_matches && !silent {
        cli_message!("{}", config);
    }

    Ok(config)
}

fn do_contacts(args: &ArgMatches, address_book: Arc<Mutex<AddressBook>>) -> Result<()> {
    let mut address_book = address_book.lock().unwrap();
    if let Some(add_args) = args.subcommand_matches("add") {
        let name = add_args.value_of("name").unwrap();
        let public_key = add_args.value_of("public-key").unwrap();
        address_book.add_contact(&Contact::new(public_key, name))?;
    } else if let Some(add_args) = args.subcommand_matches("remove") {
        let name = add_args.value_of("name").unwrap();
        address_book.remove_contact_by_name(name)?;
    } else {
        let contacts: Vec<()> = address_book
            .contact_iter()
            .map(|contact| {
                println!("@{} = {}", contact.name, contact.public_key);
                ()
            })
            .collect();

        if contacts.len() == 0 {
            println!("your contact list is empty. consider using `contacts add` to add a new contact.");
        }
    }
    Ok(())
}

fn do_listen(wallet: &mut Wallet, password: &str) -> Result<()> {
	if Wallet713Config::exists() {
		let config = Wallet713Config::from_file().map_err(|_| {
            Wallet713Error::LoadConfig
        })?;
		if config.grinbox_private_key.is_empty() {
            Err(Wallet713Error::ConfigMissingKeys)?
		} else if config.grinbox_uri.is_empty() {
            Err(Wallet713Error::ConfigMissingValue("gribox uri".to_string()))?
		} else {
            wallet.start_client(password, &config.grinbox_uri[..], &config.grinbox_private_key[..])?;
		    Ok(())
        }
	} else {
		Err(Wallet713Error::ConfigNotFound)?
	}
}

const WELCOME_HEADER: &str = r#"
Welcome to wallet713

"#;

const WELCOME_FOOTER: &str = r#"Use `listen` to connect to grinbox or `help` to see available commands
"#;

fn welcome() -> Result<Wallet713Config> {
    let config = do_config(&ArgMatches::new(), true)?;

    let secret_key = SecretKey::from_hex(&config.grinbox_private_key)?;
    let public_key = common::crypto::public_key_from_secret_key(&secret_key);
    let public_key = public_key.to_base58_check(common::crypto::BASE58_CHECK_VERSION_GRIN_TX.to_vec());

	print!("{}", WELCOME_HEADER.bright_yellow().bold());
    println!("{}: {}", "Your 713.grinbox address".bright_yellow(), public_key.bright_green());
	println!("{}", WELCOME_FOOTER.bright_blue().bold());

    Ok(config)
}

fn main() {
	let config = welcome().unwrap_or_else(|e| {
        panic!("{}: could not read or create config! {}", "ERROR".bright_red(), e);
    });

    let address_book = AddressBook::new(&config).expect("could not create an address book!");
    let address_book = Arc::new(Mutex::new(address_book));
    let mut wallet = Wallet::new(address_book.clone());

    loop {
        cli_message!();
        let mut command = String::new();
        std::io::stdin().read_line(&mut command).expect("oops!");
        let result = do_command(&command, &mut wallet, address_book.clone());
        if let Err(err) = result {
            cli_message!("{}: {}", "ERROR".bright_red(), err);
        }
    }
}

fn do_command(command: &str, wallet: &mut Wallet, address_book: Arc<Mutex<AddressBook>>) -> Result<()> {
    let account = "default".to_owned();
    let matches = Parser::parse(command)?;
    match matches.subcommand_name() {
        Some("exit") => {
            std::process::exit(0);
        },
        Some("config") => {
            do_config(matches.subcommand_matches("config").unwrap(), false)?;
        },
        Some("init") => {
            let password = matches.subcommand_matches("init").unwrap().value_of("password").unwrap_or("");
            wallet.init(password)?;
        },
        Some("listen") => {
            let password = matches.subcommand_matches("listen").unwrap().value_of("password").unwrap_or("");
            do_listen(wallet, password)?;
        },
        Some("subscribe") => {
            wallet.subscribe()?;
        },
        Some("unsubscribe") => {
            wallet.unsubscribe()?;
        },
        Some("stop") => {
            wallet.stop_client()?;
        },
        Some("info") => {
            let password = matches.subcommand_matches("info").unwrap().value_of("password").unwrap_or("");
            wallet.info(password, &account[..])?;
        },
        Some("txs") => {
            let password = matches.subcommand_matches("txs").unwrap().value_of("password").unwrap_or("");
            wallet.txs(password, &account[..])?;
        },
        Some("contacts") => {
            let arg_matches = matches.subcommand_matches("contacts").unwrap();
            do_contacts(&arg_matches, address_book.clone())?;
        },
        Some("outputs") => {
            let password = matches.subcommand_matches("outputs").unwrap().value_of("password").unwrap_or("");
            let show_spent = matches.subcommand_matches("outputs").unwrap().is_present("show-spent");
            wallet.outputs(password, &account[..], show_spent)?;
        },
        Some("repost") => {
            let password = matches.subcommand_matches("repost").unwrap().value_of("password").unwrap_or("");
            let id = matches.subcommand_matches("repost").unwrap().value_of("id").unwrap();
            let id = id.parse::<u32>().map_err(|_| {
                Wallet713Error::InvalidTxId(id.to_string())
            })?;
            wallet.repost(password, id, false)?;
        },
        Some("cancel") => {
            let password = matches.subcommand_matches("cancel").unwrap().value_of("password").unwrap_or("");
            let id = matches.subcommand_matches("cancel").unwrap().value_of("id").unwrap();
            let id = id.parse::<u32>().map_err(|_| {
                Wallet713Error::InvalidTxId(id.to_string())
            })?;
            wallet.cancel(password, id)?;
        },
        Some("send") => {
            let args = matches.subcommand_matches("send").unwrap();
            let password = args.value_of("password").unwrap_or("");
            let to = args.value_of("to").unwrap();
            let amount = args.value_of("amount").unwrap();
            let amount = core::amount_from_hr_string(amount).map_err(|_| {
                Wallet713Error::InvalidAmount(amount.to_string())
            })?;
            let slate = wallet.send(password, &account[..], to, amount, 10, "all", 1, 500)?;
            cli_message!("slate [{}] for [{}] grins sent successfully to [{}]",
                        slate.id.to_string().bright_green(),
                        core::amount_to_hr_string(slate.amount, false).bright_green(),
                        to.bright_green()
                    );
        },
        Some("restore") => {
            let password = matches.subcommand_matches("restore").unwrap().value_of("password").unwrap_or("");
            wallet.restore(password)?;
        },
        Some("challenge") => {
            cli_message!("{}", wallet.client.get_challenge());
        },
        Some(subcommand) => {
            cli_message!("{}: subcommand `{}` not implemented!", "ERROR".bright_red(), subcommand.bright_green());
        },
        None => {},
    };
    Ok(())
}