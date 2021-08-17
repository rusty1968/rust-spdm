// Copyright (c) 2021 Intel Corporation
//
// SPDX-License-Identifier: BSD-2-Clause-Patent

use fuzzlib::{*, spdmlib::{config, session::{SpdmSession, SpdmSessionState}}};

fn fuzz_handle_spdm_psk_finish(data: &[u8]) {
    let (config_info1, provision_info1) = rsp_create_info();
    let (config_info2, provision_info2) = rsp_create_info();
    let (config_info3, provision_info3) = rsp_create_info();
    let pcidoe_transport_encap = &mut PciDoeTransportEncap {};
    let mctp_transport_encap = &mut MctpTransportEncap {};



    {
        let shared_buffer = SharedBuffer::new();
        let mut socket_io_transport = FakeSpdmDeviceIoReceve::new(&shared_buffer);
        spdmlib::crypto::asym_sign::register(ASYM_SIGN_IMPL);
        let mut context = responder::ResponderContext::new(
            &mut socket_io_transport,
            if USE_PCIDOE {
                pcidoe_transport_encap
            } else {
                mctp_transport_encap
            },
            config_info1,
            provision_info1,
        );

        context.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        context.common.session = [SpdmSession::new(); 4];
        context.common.session[0].setup(4294901758).unwrap();
        context.common.session[0].set_crypto_param(
            SpdmBaseHashAlgo::TPM_ALG_SHA_384,
            SpdmDheAlgo::SECP_384_R1,
            SpdmAeadAlgo::AES_256_GCM,
            SpdmKeyScheduleAlgo::SPDM_KEY_SCHEDULE,
        );
        context.common.session[0].set_session_state(SpdmSessionState::SpdmSessionEstablished);

        context.handle_spdm_psk_finish(4294901758, data);
        let mut req_buf = [0u8; 1024];
        socket_io_transport.receive(&mut req_buf).unwrap();
    }

    {
        let shared_buffer = SharedBuffer::new();
        let mut socket_io_transport = FakeSpdmDeviceIoReceve::new(&shared_buffer);
        spdmlib::crypto::asym_sign::register(ASYM_SIGN_IMPL);
        drop(spdmlib::crypto::hmac::register);
        spdmlib::crypto::hmac::register(FUZZ_HMAC);
        let mut context = responder::ResponderContext::new(
            &mut socket_io_transport,
            if USE_PCIDOE {
                pcidoe_transport_encap
            } else {
                mctp_transport_encap
            },
            config_info2,
            provision_info2,
        );
        context.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        context.common.session = [SpdmSession::new(); 4];
        context.common.session[0].setup(4294901758).unwrap();
        context.common.session[0].set_crypto_param(
            SpdmBaseHashAlgo::TPM_ALG_SHA_384,
            SpdmDheAlgo::SECP_384_R1,
            SpdmAeadAlgo::AES_256_GCM,
            SpdmKeyScheduleAlgo::SPDM_KEY_SCHEDULE,
        );
        context.common.session[0].set_session_state(SpdmSessionState::SpdmSessionEstablished);
        context.handle_spdm_psk_finish(4294901758, data);
        let mut req_buf = [0u8; 1024];
        socket_io_transport.receive(&mut req_buf).unwrap();
    }
    {
        let shared_buffer = SharedBuffer::new();
        let mut socket_io_transport = FakeSpdmDeviceIoReceve::new(&shared_buffer);
        spdmlib::crypto::asym_sign::register(ASYM_SIGN_IMPL);
        spdmlib::crypto::hmac::register(FUZZ_HMAC);

        let mut context = responder::ResponderContext::new(
            &mut socket_io_transport,
            if USE_PCIDOE {
                pcidoe_transport_encap
            } else {
                mctp_transport_encap
            },
            config_info3,
            provision_info3,
        );
        context.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        context.common.session = [SpdmSession::new(); 4];
        context.common.session[0].setup(4294901758).unwrap();
        context.common.session[0].set_crypto_param(
            SpdmBaseHashAlgo::TPM_ALG_SHA_384,
            SpdmDheAlgo::SECP_384_R1,
            SpdmAeadAlgo::AES_256_GCM,
            SpdmKeyScheduleAlgo::SPDM_KEY_SCHEDULE,
        );
        context.common.runtime_info.message_a.append_message(&[1u8; config::MAX_SPDM_MESSAGE_BUFFER_SIZE]);
        context.common.session[0].set_session_state(SpdmSessionState::SpdmSessionEstablished);
        context.handle_spdm_psk_finish(4294901758, data);
        let mut req_buf = [0u8; 1024];
        socket_io_transport.receive(&mut req_buf).unwrap();
    }
}
fn main() {
    #[cfg(feature = "fuzzlog")]
    flexi_logger::Logger::try_with_str("info")
        .unwrap()
        .log_to_file(
            FileSpec::default()
                .directory("traces")
                .basename("foo")
                .discriminant("Sample4711A")
                .suffix("trc"),
        )
        .print_message()
        .create_symlink("current_run")
        .start()
        .unwrap();
    // if cfg!(feature = "analysis") {
    //     let args: Vec<String> = std::env::args().collect();
    //     println!("{:?}", args);
    //     if args.len() < 2 {
    //         println!("Please enter the path of the crash file as the first parameter");
    //         return;
    //     }
    //     let path = &args[1];
    //     let data = std::fs::read(path).expect("read crash file fail");
    //     fuzz_handle_spdm_psk_finish(data.as_slice());
    // } else {
    //     afl::fuzz!(|data: &[u8]| {
    //         fuzz_handle_spdm_psk_finish(data);
    //     });
    // }
    fuzz_handle_spdm_psk_finish(&[1u8; 92]);

}
