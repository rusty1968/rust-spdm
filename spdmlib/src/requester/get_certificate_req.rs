// Copyright (c) 2020 Intel Corporation
//
// SPDX-License-Identifier: BSD-2-Clause-Patent

use crate::crypto;
use crate::error::{
    SpdmResult, SPDM_STATUS_BUFFER_FULL, SPDM_STATUS_CRYPTO_ERROR, SPDM_STATUS_ERROR_PEER,
    SPDM_STATUS_INVALID_CERT, SPDM_STATUS_INVALID_MSG_FIELD, SPDM_STATUS_INVALID_PARAMETER,
    SPDM_STATUS_INVALID_STATE_LOCAL,
};
use crate::message::*;
use crate::protocol::*;
use crate::requester::*;

impl<'a> RequesterContext<'a> {
    fn send_receive_spdm_certificate_partial(
        &mut self,
        session_id: Option<u32>,
        slot_id: u8,
        total_size: u16,
        offset: u16,
        length: u16,
    ) -> SpdmResult<(u16, u16)> {
        info!("send spdm certificate\n");
        let mut send_buffer = [0u8; config::MAX_SPDM_MESSAGE_BUFFER_SIZE];
        let send_used =
            self.encode_spdm_certificate_partial(slot_id, offset, length, &mut send_buffer);
        if send_used == 0 {
            return Err(SPDM_STATUS_BUFFER_FULL);
        }
        match session_id {
            Some(session_id) => {
                self.send_secured_message(session_id, &send_buffer[..send_used], false)?;
            }
            None => {
                self.send_message(&send_buffer[..send_used])?;
            }
        }

        let mut receive_buffer = [0u8; config::MAX_SPDM_MESSAGE_BUFFER_SIZE];
        let used = match session_id {
            Some(session_id) => {
                self.receive_secured_message(session_id, &mut receive_buffer, false)?
            }
            None => self.receive_message(&mut receive_buffer, false)?,
        };

        self.handle_spdm_certificate_partial_response(
            session_id,
            slot_id,
            total_size,
            (offset, length),
            &send_buffer[..send_used],
            &receive_buffer[..used],
        )
    }

    pub fn encode_spdm_certificate_partial(
        &mut self,
        slot_id: u8,
        offset: u16,
        length: u16,
        buf: &mut [u8],
    ) -> usize {
        let mut writer = Writer::init(buf);
        let request = SpdmMessage {
            header: SpdmMessageHeader {
                version: self.common.negotiate_info.spdm_version_sel,
                request_response_code: SpdmRequestResponseCode::SpdmRequestGetCertificate,
            },
            payload: SpdmMessagePayload::SpdmGetCertificateRequest(
                SpdmGetCertificateRequestPayload {
                    slot_id,
                    offset,
                    length,
                },
            ),
        };
        if let Ok(sz) = request.spdm_encode(&mut self.common, &mut writer) {
            sz
        } else {
            0
        }
    }

    pub fn handle_spdm_certificate_partial_response(
        &mut self,
        session_id: Option<u32>,
        slot_id: u8,
        total_size: u16,
        param: (u16, u16),
        send_buffer: &[u8],
        receive_buffer: &[u8],
    ) -> SpdmResult<(u16, u16)> {
        let offset = param.0;
        let length = param.1;
        let mut reader = Reader::init(receive_buffer);
        match SpdmMessageHeader::read(&mut reader) {
            Some(message_header) => {
                if message_header.version != self.common.negotiate_info.spdm_version_sel {
                    return Err(SPDM_STATUS_INVALID_MSG_FIELD);
                }
                match message_header.request_response_code {
                    SpdmRequestResponseCode::SpdmResponseCertificate => {
                        let certificate = SpdmCertificateResponsePayload::spdm_read(
                            &mut self.common,
                            &mut reader,
                        );
                        let used = reader.used();
                        if let Some(certificate) = certificate {
                            debug!("!!! certificate : {:02x?}\n", certificate);
                            if certificate.portion_length as usize > length as usize
                                || (offset + certificate.portion_length) as usize
                                    > config::MAX_SPDM_CERT_CHAIN_DATA_SIZE
                            {
                                return Err(SPDM_STATUS_INVALID_MSG_FIELD);
                            }
                            if total_size != 0
                                && total_size
                                    != offset
                                        + certificate.portion_length
                                        + certificate.remainder_length
                            {
                                return Err(SPDM_STATUS_INVALID_MSG_FIELD);
                            }
                            if certificate.slot_id != slot_id {
                                error!("slot id is not match between requester and responder!\n");
                                return Err(SPDM_STATUS_INVALID_MSG_FIELD);
                            }
                            if self.common.peer_info.peer_cert_chain[slot_id as usize].is_none() {
                                if offset != 0 {
                                    error!("offset invalid!\n");
                                    return Err(SPDM_STATUS_INVALID_MSG_FIELD);
                                }
                                self.common.peer_info.peer_cert_chain[slot_id as usize] =
                                    Some(SpdmCertChain::default());
                            }
                            self.common.peer_info.peer_cert_chain[slot_id as usize]
                                .as_mut()
                                .ok_or(SPDM_STATUS_INVALID_STATE_LOCAL)?
                                .cert_chain
                                .data[(offset as usize)
                                ..(offset as usize + certificate.portion_length as usize)]
                                .copy_from_slice(
                                    &certificate.cert_chain
                                        [0..(certificate.portion_length as usize)],
                                );

                            self.common.peer_info.peer_cert_chain[slot_id as usize]
                                .as_mut()
                                .ok_or(SPDM_STATUS_INVALID_STATE_LOCAL)?
                                .cert_chain
                                .data_size = offset + certificate.portion_length;

                            match session_id {
                                None => {
                                    #[cfg(not(feature = "hashed-transcript-data"))]
                                    {
                                        let message_b = &mut self.common.runtime_info.message_b;
                                        message_b.append_message(send_buffer).map_or_else(
                                            || Err(SPDM_STATUS_BUFFER_FULL),
                                            |_| Ok(()),
                                        )?;
                                        message_b
                                            .append_message(&receive_buffer[..used])
                                            .map_or_else(
                                                || Err(SPDM_STATUS_BUFFER_FULL),
                                                |_| Ok(()),
                                            )?;
                                    }

                                    #[cfg(feature = "hashed-transcript-data")]
                                    {
                                        crypto::hash::hash_ctx_update(
                                            self.common
                                                .runtime_info
                                                .digest_context_m1m2
                                                .as_mut()
                                                .ok_or(SPDM_STATUS_INVALID_STATE_LOCAL)?,
                                            send_buffer,
                                        )?;
                                        crypto::hash::hash_ctx_update(
                                            self.common
                                                .runtime_info
                                                .digest_context_m1m2
                                                .as_mut()
                                                .ok_or(SPDM_STATUS_INVALID_STATE_LOCAL)?,
                                            &receive_buffer[..used],
                                        )?;
                                    }
                                }
                                Some(_session_id) => {}
                            }

                            Ok((certificate.portion_length, certificate.remainder_length))
                        } else {
                            error!("!!! certificate : fail !!!\n");
                            Err(SPDM_STATUS_INVALID_MSG_FIELD)
                        }
                    }
                    SpdmRequestResponseCode::SpdmResponseError => {
                        let rm = self.spdm_handle_error_response_main(
                            session_id,
                            receive_buffer,
                            SpdmRequestResponseCode::SpdmRequestGetCertificate,
                            SpdmRequestResponseCode::SpdmResponseCertificate,
                        )?;
                        let receive_buffer = rm.receive_buffer;
                        let used = rm.used;
                        self.handle_spdm_certificate_partial_response(
                            session_id,
                            slot_id,
                            total_size,
                            (offset, length),
                            send_buffer,
                            &receive_buffer[..used],
                        )
                    }
                    _ => Err(SPDM_STATUS_ERROR_PEER),
                }
            }
            None => Err(SPDM_STATUS_INVALID_MSG_FIELD),
        }
    }

    pub fn send_receive_spdm_certificate(
        &mut self,
        session_id: Option<u32>,
        slot_id: u8,
    ) -> SpdmResult {
        let mut offset = 0u16;
        let mut length = MAX_SPDM_CERT_PORTION_LEN as u16;
        let mut total_size = 0u16;
        while length != 0 {
            let (portion_length, remainder_length) = self.send_receive_spdm_certificate_partial(
                session_id, slot_id, total_size, offset, length,
            )?;
            if total_size == 0 {
                total_size = portion_length + remainder_length;
            }
            offset += portion_length;
            length = remainder_length;
            if length > MAX_SPDM_CERT_PORTION_LEN as u16 {
                length = MAX_SPDM_CERT_PORTION_LEN as u16;
            }
        }
        if total_size != 0 {
            self.verify_spdm_certificate_chain(slot_id)
        } else {
            Err(SPDM_STATUS_INVALID_CERT)
        }
    }

    pub fn verify_spdm_certificate_chain(&mut self, slot_id: u8) -> SpdmResult {
        // verify
        if let Some(peer_cert_chain_data) = &self.common.provision_info.peer_cert_chain_data {
            //
            // Verify cert chain
            //
            if self.common.peer_info.peer_cert_chain[slot_id as usize].is_none() {
                error!("peer_cert_chain is not populated!\n");
                return Err(SPDM_STATUS_INVALID_PARAMETER);
            }
            if self.common.peer_info.peer_cert_chain[slot_id as usize]
                .as_ref()
                .ok_or(SPDM_STATUS_INVALID_PARAMETER)?
                .cert_chain
                .data_size
                <= (4 + self.common.negotiate_info.base_hash_sel.get_size())
            {
                return Err(SPDM_STATUS_INVALID_CERT);
            }

            let data_size = self.common.peer_info.peer_cert_chain[slot_id as usize]
                .as_ref()
                .ok_or(SPDM_STATUS_INVALID_PARAMETER)?
                .cert_chain
                .data_size
                - 4
                - self.common.negotiate_info.base_hash_sel.get_size();
            let mut data = [0u8; config::MAX_SPDM_CERT_CHAIN_DATA_SIZE];
            data[0..(data_size as usize)].copy_from_slice(
                &self.common.peer_info.peer_cert_chain[slot_id as usize]
                    .as_ref()
                    .ok_or(SPDM_STATUS_INVALID_PARAMETER)?
                    .cert_chain
                    .data[(4usize
                    + self.common.negotiate_info.base_hash_sel.get_size() as usize)
                    ..(self.common.peer_info.peer_cert_chain[slot_id as usize]
                        .as_ref()
                        .ok_or(SPDM_STATUS_INVALID_PARAMETER)?
                        .cert_chain
                        .data_size as usize)],
            );
            let runtime_peer_cert_chain_data = SpdmCertChainData { data_size, data };

            let (root_cert_begin, root_cert_end) =
                crypto::cert_operation::get_cert_from_cert_chain(
                    &runtime_peer_cert_chain_data.data
                        [..(runtime_peer_cert_chain_data.data_size as usize)],
                    0,
                )?;
            let root_cert = &runtime_peer_cert_chain_data.data[root_cert_begin..root_cert_end];
            let root_hash = if let Some(rh) =
                crypto::hash::hash_all(self.common.negotiate_info.base_hash_sel, root_cert)
            {
                rh
            } else {
                return Err(SPDM_STATUS_CRYPTO_ERROR);
            };
            if root_hash.data[..(root_hash.data_size as usize)]
                != self.common.peer_info.peer_cert_chain[slot_id as usize]
                    .as_ref()
                    .ok_or(SPDM_STATUS_INVALID_PARAMETER)?
                    .cert_chain
                    .data[4usize
                    ..(4usize + self.common.negotiate_info.base_hash_sel.get_size() as usize)]
            {
                error!("root_hash - fail!\n");
                return Err(SPDM_STATUS_INVALID_CERT);
            }

            if runtime_peer_cert_chain_data.data_size != peer_cert_chain_data.data_size {
                error!("cert_chain size - fail!\n");
                debug!(
                    "provision cert_chain data size - {:?}\n",
                    peer_cert_chain_data.data_size
                );
                debug!(
                    "runtime cert_chain data size - {:?}\n",
                    runtime_peer_cert_chain_data.data_size
                );
                return Err(SPDM_STATUS_INVALID_CERT);
            }
            if runtime_peer_cert_chain_data.data != peer_cert_chain_data.data {
                error!("cert_chain data - fail!\n");
                return Err(SPDM_STATUS_INVALID_CERT);
            }

            if crypto::cert_operation::verify_cert_chain(
                &runtime_peer_cert_chain_data.data
                    [..(runtime_peer_cert_chain_data.data_size as usize)],
            )
            .is_err()
            {
                error!("cert_chain verification - fail! - TBD later\n");
                return Err(SPDM_STATUS_INVALID_CERT);
            }
            info!("cert_chain verification - pass!\n");
        }
        Ok(())
    }
}

#[cfg(all(test,))]
mod tests_requester {
    use super::*;
    use crate::testlib::*;
    use crate::{crypto, responder};

    #[test]
    fn test_case0_send_receive_spdm_certificate() {
        let (rsp_config_info, rsp_provision_info) = create_info();
        let (req_config_info, req_provision_info) = create_info();

        let shared_buffer = SharedBuffer::new();
        let mut device_io_responder = FakeSpdmDeviceIoReceve::new(&shared_buffer);

        let pcidoe_transport_encap = &mut PciDoeTransportEncap {};

        crypto::asym_sign::register(ASYM_SIGN_IMPL.clone());

        let mut responder = responder::ResponderContext::new(
            &mut device_io_responder,
            pcidoe_transport_encap,
            rsp_config_info,
            rsp_provision_info,
        );

        responder.common.reset_runtime_info();
        responder.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        responder.common.negotiate_info.base_asym_sel =
            SpdmBaseAsymAlgo::TPM_ALG_ECDSA_ECC_NIST_P384;
        responder.common.provision_info.my_cert_chain = Some(REQ_CERT_CHAIN_DATA);
        responder.common.runtime_info.digest_context_m1m2 = Some(
            crypto::hash::hash_ctx_init(responder.common.negotiate_info.base_hash_sel).unwrap(),
        );

        let pcidoe_transport_encap2 = &mut PciDoeTransportEncap {};
        let mut device_io_requester = FakeSpdmDeviceIo::new(&shared_buffer, &mut responder);

        let mut requester = RequesterContext::new(
            &mut device_io_requester,
            pcidoe_transport_encap2,
            req_config_info,
            req_provision_info,
        );

        requester.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        requester.common.negotiate_info.base_asym_sel =
            SpdmBaseAsymAlgo::TPM_ALG_ECDSA_ECC_NIST_P384;
        requester.common.runtime_info.digest_context_m1m2 = Some(
            crypto::hash::hash_ctx_init(requester.common.negotiate_info.base_hash_sel).unwrap(),
        );

        let status = requester.send_receive_spdm_certificate(None, 0).is_ok();
        assert!(status);
    }
}
