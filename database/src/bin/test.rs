use std::{
    fs::File,
    io::{BufReader, BufWriter},
};

use capnp::message::Builder;
use message_capnp::person::PhoneType;

// Import the generated module
pub mod message_capnp {
    include!(concat!(env!("OUT_DIR"), "/schema/message_capnp.rs"));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new message
    let mut message = Builder::new_default();

    // Create a person
    let mut person = message.init_root::<message_capnp::person::Builder>();
    person.set_id(123);
    person.set_name("John Doe");
    person.set_email("john@example.com");

    // Add two phone numbers
    let mut phones = person.init_phones(2);
    {
        let mut phone = phones.reborrow().get(0);
        phone.set_number("555-1234");
        phone.set_type(PhoneType::Mobile);
    }
    let mut phone_2 = phones.reborrow().get(1);
    phone_2.set_number("123-456");
    phone_2.set_type(PhoneType::Work);

    // Write to disk
    {
        let file = File::create("person.data")?;
        let mut writer = BufWriter::new(file);
        capnp::serialize::write_message(&mut writer, &message)?;
    }

    println!("Successfully wrote message to person.capnp");

    // Read from disk
    {
        // NOTE THAT THIS ISN'T THE FILE THAT WAS JUST WRITTEN
        let file = File::open("person.capnp")?;
        let reader = BufReader::new(file);

        let message_reader =
            capnp::serialize::read_message(reader, capnp::message::ReaderOptions::default())?;

        let person_reader = message_reader.get_root::<message_capnp::person::Reader>()?;
        println!("ID: {}", person_reader.get_id());
        println!("Name: {:?}", person_reader.get_name()?);
        println!("Email: {:?}", person_reader.get_email()?);

        let phones_reader = person_reader.get_phones()?;
        for (idx, phone) in phones_reader.into_iter().enumerate() {
            println!("idx: {idx}");
            println!("Phone: {:?}", phone.get_number()?);
            println!("Type: {:?}", phone.get_type());
        }
    }

    Ok(())
}
