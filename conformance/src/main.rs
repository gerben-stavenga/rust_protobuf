#![feature(allocator_api)]

use anyhow::{Context, Result, bail};
use protocrap::ProtobufExt;
use protocrap::reflection::DynamicMessage;
use protocrap_conformance::conformance::{ConformanceRequest, ConformanceResponse, WireFormat};
use protocrap_conformance::protobuf_test_messages::proto2::TestAllTypesProto2;
use protocrap_conformance::protobuf_test_messages::proto3::TestAllTypesProto3;
use std::io::{self, Read, Write};

const TEST_JSON: bool = true;

fn roundtrip_proto<T: protocrap::ProtobufExt>(
    arena: &mut protocrap::arena::Arena,
    request: &ConformanceRequest::ProtoType,
) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();
    let mut msg = T::default();
    // Decode input
    if let Some(data) = request.get_protobuf_payload() {
        if !msg.decode_flat::<32>(arena, data) {
            response.set_parse_error("Failed to parse protobuf message", arena);
            return response;
        }
    } else if let Some(data) = request.get_json_payload() {
        if !TEST_JSON {
            response.set_skipped("Json format input not supported", arena);
            return response;
        }
        if let Err(e) = msg.serde_deserialize(arena, &mut serde_json::Deserializer::from_str(data))
        {
            response.set_parse_error(&format!("Failed to parse JSON message: {:?}", e), arena);
            return response;
        }
    } else if request.has_text_payload() {
        response.set_skipped("Text format input not supported", arena);
        return response;
    } else if request.has_jspb_payload() {
        response.set_skipped("JSPB input not supported", arena);
        return response;
    } else {
        response.set_runtime_error("No input payload specified", arena);
        return response;
    };
    // Encode output
    match request.requested_output_format() {
        Some(WireFormat::PROTOBUF) => match msg.encode_vec::<32>() {
            Ok(bytes) => {
                response.set_protobuf_payload(&bytes, arena);
            }
            Err(e) => {
                response.set_serialize_error(&format!("Encode error: {:?}", e), arena);
            }
        },
        Some(WireFormat::JSON) => {
            if !TEST_JSON {
                response.set_skipped("Json format output not supported", arena);
                return response;
            }
            let mut serializer = serde_json::Serializer::new(Vec::new());
            let dynamic_msg = DynamicMessage::new(&msg);
            use serde::ser::Serialize;
            match dynamic_msg.serialize(&mut serializer) {
                Ok(()) => {
                    let vec = serializer.into_inner();
                    let json_str = String::from_utf8(vec).unwrap_or_default();
                    response.set_json_payload(&json_str, arena);
                }
                Err(e) => {
                    response.set_serialize_error(&format!("JSON serialize error: {:?}", e), arena);
                }
            }
        }
        Some(WireFormat::TEXT_FORMAT) => {
            response.set_skipped("Text format output not supported", arena);
        }
        Some(WireFormat::JSPB) => {
            response.set_skipped("JSPB output not supported", arena);
        }
        None | Some(WireFormat::UNSPECIFIED) => {
            response.set_skipped("Output format unspecified", arena);
        }
    }
    response
}

fn do_test(
    request: &ConformanceRequest::ProtoType,
    arena: &mut protocrap::arena::Arena,
) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();

    let message_type = request.message_type();
    let is_proto3 = message_type.contains("Proto3") || message_type.is_empty();

    if is_proto3 {
        roundtrip_proto::<TestAllTypesProto3::ProtoType>(arena, request)
    } else {
        if message_type != "protobuf_test_messages.proto2.TestAllTypesProto2" {
            response.set_skipped(
                &format!("Message type {} not supported", message_type),
                arena,
            );
            return response;
        }
        roundtrip_proto::<TestAllTypesProto2::ProtoType>(arena, request)
    }
}

fn main() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    eprintln!("Protocrap conformance test runner starting...");

    loop {
        let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
        // Read message length
        use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
        let Ok(len) = stdin
            .read_u32::<byteorder::LittleEndian>()
            .context("Failed to read message length")
        else {
            break;
        };
        // eprintln!("Processing test #{} ({} bytes)", test_count + 1, len);

        // Read request message
        let mut request_bytes = vec![0u8; len as usize];
        stdin
            .read_exact(&mut request_bytes)
            .context("Failed to read request")?;

        // Parse ConformanceRequest
        let mut request = ConformanceRequest::ProtoType::default();
        if !request.decode_flat::<32>(&mut arena, &request_bytes) {
            bail!("Failed to decode ConformanceRequest");
        }
        let response = do_test(&request, &mut arena);

        // Serialize ConformanceResponse
        let response_bytes = response
            .encode_vec::<32>()
            .context("Failed to encode response")?;

        // eprintln!("Response {:?} finished test #{}", response, test_count + 1);

        // Write response length and message
        stdout.write_u32::<LittleEndian>(response_bytes.len() as u32)?;
        stdout.write_all(&response_bytes)?;
        stdout.flush()?;
    }

    Ok(())
}
