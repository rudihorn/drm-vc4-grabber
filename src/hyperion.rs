use std::{io::{Cursor, Read, Write}, net::TcpStream};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use flatbuffers::FlatBufferBuilder;
use image::{EncodableLayout, RgbImage};

use std::io::Result as StdResult;

use crate::hyperion_reply_generated::hyperionnet as reply;
use crate::hyperion_request_generated::hyperionnet as request;

pub fn read_reply(socket: &mut TcpStream) -> StdResult<()> {
    let mut size = [0u8; 4];
    socket.read_exact(&mut size)?;

    let v = Cursor::new(size).read_u32::<BigEndian>()? as usize;
    println!("Size: {}", v);
    let mut msg = vec![0; v];
    socket.read_exact(&mut msg)?;

    let request = reply::root_as_reply(&msg).unwrap();

    println!("Response {:?}", request);

    Ok(())
}

pub fn register_direct(socket: &mut TcpStream) -> StdResult<()> {
    let mut builder = FlatBufferBuilder::new();

    let origin = builder.create_string("DRM");
    let register = request::Register::create(
        &mut builder,
        &request::RegisterArgs {
            origin: Some(origin),
            priority: 150,
        },
    );
    let offset = request::Request::create(
        &mut builder,
        &request::RequestArgs {
            command_type: request::Command::Register,
            command: Some(register.as_union_value()),
        },
    );
    request::finish_request_buffer(&mut builder, offset);

    let dat = builder.finished_data();

    socket.write_u32::<BigEndian>(dat.len() as _)?;
    socket.write_all(dat)?;
    socket.flush()?;

    Ok(())
}

pub fn send_image(socket: &mut TcpStream, image: & RgbImage) -> StdResult<()> {
    let mut builder = FlatBufferBuilder::new();

    let raw_bytes = image.as_bytes();

    println!(
        "Sending image {}x{} (size: {})",
        image.width(),
        image.height(),
        raw_bytes.len()
    );

    let mut image_bytes = vec![0;raw_bytes.len()];
    for i in 0..image_bytes.len()/3 {
        let pos = i*3;
        image_bytes[pos] = raw_bytes[pos+2];
        image_bytes[pos+1] = raw_bytes[pos+1];
        image_bytes[pos+2] = raw_bytes[pos];
    }

    let data = builder.create_vector(&raw_bytes);
    let raw_image = request::RawImage::create(
        &mut builder,
        &request::RawImageArgs {
            data: Some(data),
            width: image.width() as _,
            height: image.height() as _,
        },
    );

    let image = request::Image::create(
        &mut builder,
        &request::ImageArgs {
            data_type: request::ImageType::RawImage,
            data: Some(raw_image.as_union_value()),
            duration: 1000,
        },
    );

    let offset = request::Request::create(
        &mut builder,
        &request::RequestArgs {
            command_type: request::Command::Image,
            command: Some(image.as_union_value()),
        },
    );

    request::finish_request_buffer(&mut builder, offset);

    let dat = builder.finished_data();
    socket.write_u32::<BigEndian>(dat.len() as _)?;
    socket.write_all(dat)?;
    socket.flush()?;

    read_reply(socket)?;

    Ok(())
}

pub fn send_color_red(socket: &mut TcpStream) -> StdResult<()> {
    println!("Setting color");
    let mut builder = flatbuffers::FlatBufferBuilder::new();

    let color = request::Color::create(
        &mut builder,
        &request::ColorArgs {
            data: 0x00100000,
            duration: 5000,
        },
    );

    let offset = request::Request::create(
        &mut builder,
        &request::RequestArgs {
            command_type: request::Command::Color,
            command: Some(color.as_union_value()),
        },
    );

    request::finish_request_buffer(&mut builder, offset);

    let dat = builder.finished_data();
    socket.write_u32::<BigEndian>(dat.len() as _)?;
    socket.write_all(dat)?;
    socket.flush()?;

    read_reply(socket)?;

    Ok(())
}
