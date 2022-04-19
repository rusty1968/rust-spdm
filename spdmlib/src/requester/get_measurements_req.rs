// Copyright (c) 2020 Intel Corporation
//
// SPDX-License-Identifier: BSD-2-Clause-Patent

use crate::common::error::SpdmResult;
use crate::crypto;
use crate::message::*;
use crate::requester::*;

impl<'a> RequesterContext<'a> {
    fn send_receive_spdm_measurement_record(
        &mut self,
        session_id: Option<u32>,
        measurement_attributes: SpdmMeasurementeAttributes,
        measurement_operation: SpdmMeasurementOperation,
        slot_id: u8,
    ) -> SpdmResult<u8> {
        info!("send spdm measurement\n");
        let mut send_buffer = [0u8; config::MAX_SPDM_MESSAGE_BUFFER_SIZE];
        let send_used = self.encode_spdm_measurement_record(
            measurement_attributes,
            measurement_operation,
            slot_id,
            &mut send_buffer,
        )?;
        match session_id {
            Some(session_id) => {
                self.send_secured_message(session_id, &send_buffer[..send_used], false)?;
            }
            None => {
                self.send_message(&send_buffer[..send_used])?;
            }
        }

        // Receive
        let mut receive_buffer = [0u8; config::MAX_SPDM_MESSAGE_BUFFER_SIZE];
        let used = match session_id {
            Some(session_id) => {
                self.receive_secured_message(session_id, &mut receive_buffer, true)?
            }
            None => self.receive_message(&mut receive_buffer, true)?,
        };

        self.handle_spdm_measurement_record_response(
            session_id,
            measurement_attributes,
            measurement_operation,
            &send_buffer[..send_used],
            &receive_buffer[..used],
        )
    }

    pub fn encode_spdm_measurement_record(
        &mut self,
        measurement_attributes: SpdmMeasurementeAttributes,
        measurement_operation: SpdmMeasurementOperation,
        slot_id: u8,
        buf: &mut [u8],
    ) -> SpdmResult<usize> {
        let mut writer = Writer::init(buf);
        let mut nonce = [0u8; SPDM_NONCE_SIZE];
        crypto::rand::get_random(&mut nonce)?;

        let request = SpdmMessage {
            header: SpdmMessageHeader {
                version: self.common.negotiate_info.spdm_version_sel,
                request_response_code: SpdmRequestResponseCode::SpdmRequestGetMeasurements,
            },
            payload: SpdmMessagePayload::SpdmGetMeasurementsRequest(
                SpdmGetMeasurementsRequestPayload {
                    measurement_attributes,
                    measurement_operation,
                    nonce: SpdmNonceStruct { data: nonce },
                    slot_id,
                },
            ),
        };
        request.spdm_encode(&mut self.common, &mut writer);
        Ok(writer.used())
    }

    pub fn handle_spdm_measurement_record_response(
        &mut self,
        session_id: Option<u32>,
        measurement_attributes: SpdmMeasurementeAttributes,
        measurement_operation: SpdmMeasurementOperation,
        send_buffer: &[u8],
        receive_buffer: &[u8],
    ) -> SpdmResult<u8> {
        if measurement_attributes.contains(SpdmMeasurementeAttributes::SIGNATURE_REQUESTED) {
            self.common.runtime_info.need_measurement_signature = true;
        } else {
            self.common.runtime_info.need_measurement_signature = false;
        }

        let mut reader = Reader::init(receive_buffer);
        match SpdmMessageHeader::read(&mut reader) {
            Some(message_header) => match message_header.request_response_code {
                SpdmRequestResponseCode::SpdmResponseMeasurements => {
                    let measurements =
                        SpdmMeasurementsResponsePayload::spdm_read(&mut self.common, &mut reader);
                    let used = reader.used();
                    if let Some(measurements) = measurements {
                        debug!("!!! measurements : {:02x?}\n", measurements);

                        if message_header.version == SpdmVersion::SpdmVersion12 {
                            self.common.runtime_info.content_changed = measurements.content_changed;
                        }

                        // verify signature
                        if measurement_attributes
                            .contains(SpdmMeasurementeAttributes::SIGNATURE_REQUESTED)
                        {
                            let base_asym_size =
                                self.common.negotiate_info.base_asym_sel.get_size() as usize;
                            let temp_used = used - base_asym_size;

                            let message_m = match session_id {
                                Some(session_id) => {
                                    let session =
                                        self.common.get_session_via_id(session_id).unwrap();
                                    &mut session.runtime_info.message_m
                                }
                                None => &mut self.common.runtime_info.message_m,
                            };
                            message_m
                                .append_message(send_buffer)
                                .map_or_else(|| spdm_result_err!(ENOMEM), |_| Ok(()))?;
                            message_m
                                .append_message(&receive_buffer[..temp_used])
                                .map_or_else(|| spdm_result_err!(ENOMEM), |_| Ok(()))?;
                            if self
                                .common
                                .verify_measurement_signature(session_id, &measurements.signature)
                                .is_err()
                            {
                                error!("verify_measurement_signature fail");
                                return spdm_result_err!(EFAULT);
                            } else {
                                info!("verify_measurement_signature pass");
                            }
                            match session_id {
                                Some(session_id) => {
                                    let session =
                                        self.common.get_session_via_id(session_id).unwrap();
                                    session.runtime_info.message_m.reset_message();
                                }
                                None => self.common.runtime_info.message_m.reset_message(),
                            };
                        } else {
                            let message_m = match session_id {
                                Some(session_id) => {
                                    let session =
                                        self.common.get_session_via_id(session_id).unwrap();
                                    &mut session.runtime_info.message_m
                                }
                                None => &mut self.common.runtime_info.message_m,
                            };
                            message_m
                                .append_message(send_buffer)
                                .map_or_else(|| spdm_result_err!(ENOMEM), |_| Ok(()))?;
                            message_m
                                .append_message(&receive_buffer[..used])
                                .map_or_else(|| spdm_result_err!(ENOMEM), |_| Ok(()))?;
                        }

                        match measurement_operation {
                            SpdmMeasurementOperation::SpdmMeasurementQueryTotalNumber => {
                                Ok(measurements.number_of_measurement)
                            }
                            SpdmMeasurementOperation::SpdmMeasurementRequestAll => {
                                Ok(measurements.measurement_record.number_of_blocks)
                            }
                            _ => Ok(measurements.measurement_record.number_of_blocks),
                        }
                    } else {
                        error!("!!! measurements : fail !!!\n");
                        spdm_result_err!(EFAULT)
                    }
                }
                SpdmRequestResponseCode::SpdmResponseError => {
                    let sid: u32;
                    if let Some(sid_) = session_id {
                        sid = sid_;
                    } else {
                        sid = 0;
                    }
                    let erm = self.spdm_handle_error_response_main(
                        sid,
                        receive_buffer,
                        SpdmRequestResponseCode::SpdmRequestGetDigests,
                        SpdmRequestResponseCode::SpdmResponseDigests,
                    );
                    match erm {
                        Ok(rm) => {
                            let receive_buffer = rm.receive_buffer;
                            let used = rm.used;
                            self.handle_spdm_measurement_record_response(
                                session_id,
                                measurement_attributes,
                                measurement_operation,
                                send_buffer,
                                &receive_buffer[..used],
                            )
                        }
                        _ => spdm_result_err!(EINVAL),
                    }
                }
                _ => spdm_result_err!(EINVAL),
            },
            None => spdm_result_err!(EIO),
        }
    }

    pub fn send_receive_spdm_measurement(
        &mut self,
        session_id: Option<u32>,
        spdm_measuremente_attributes: SpdmMeasurementeAttributes,
        measurement_operation: SpdmMeasurementOperation,
        out_total_number: &mut u8, // out, total number when measurement_operation = SpdmMeasurementQueryTotalNumber
        //      number of blocks got measured.
        slot_id: u8,
    ) -> SpdmResult {
        if let Ok(total_number) = self.send_receive_spdm_measurement_record(
            session_id,
            spdm_measuremente_attributes,
            measurement_operation,
            slot_id,
        ) {
            *out_total_number = total_number;
            Ok(())
        } else {
            spdm_result_err!(EFAULT)
        }
    }
}

#[cfg(test)]
mod tests_requester {
    use super::*;
    use crate::testlib::*;
    use crate::{crypto, responder};

    #[test]
    #[should_panic(expected = "not implemented")]
    fn test_case0_send_receive_spdm_measurement() {
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

        responder.common.negotiate_info.req_ct_exponent_sel = 0;
        responder.common.negotiate_info.req_capabilities_sel = SpdmRequestCapabilityFlags::CERT_CAP;

        responder.common.negotiate_info.rsp_ct_exponent_sel = 0;
        responder.common.negotiate_info.rsp_capabilities_sel =
            SpdmResponseCapabilityFlags::CERT_CAP;

        responder
            .common
            .negotiate_info
            .measurement_specification_sel = SpdmMeasurementSpecification::DMTF;

        responder.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        responder.common.negotiate_info.base_asym_sel =
            SpdmBaseAsymAlgo::TPM_ALG_ECDSA_ECC_NIST_P384;
        responder.common.negotiate_info.measurement_hash_sel =
            SpdmMeasurementHashAlgo::TPM_ALG_SHA_384;
        let message_m = &[0];
        responder
            .common
            .runtime_info
            .message_m
            .append_message(message_m);
        responder.common.reset_runtime_info();

        let pcidoe_transport_encap2 = &mut PciDoeTransportEncap {};
        let mut device_io_requester = FakeSpdmDeviceIo::new(&shared_buffer, &mut responder);

        let mut requester = RequesterContext::new(
            &mut device_io_requester,
            pcidoe_transport_encap2,
            req_config_info,
            req_provision_info,
        );

        requester.common.negotiate_info.req_ct_exponent_sel = 0;
        requester.common.negotiate_info.req_capabilities_sel = SpdmRequestCapabilityFlags::CERT_CAP;

        requester.common.negotiate_info.rsp_ct_exponent_sel = 0;
        requester.common.negotiate_info.rsp_capabilities_sel =
            SpdmResponseCapabilityFlags::CERT_CAP;
        requester
            .common
            .negotiate_info
            .measurement_specification_sel = SpdmMeasurementSpecification::DMTF;
        requester.common.negotiate_info.base_hash_sel = SpdmBaseHashAlgo::TPM_ALG_SHA_384;
        requester.common.negotiate_info.base_asym_sel =
            SpdmBaseAsymAlgo::TPM_ALG_ECDSA_ECC_NIST_P384;
        requester.common.negotiate_info.measurement_hash_sel =
            SpdmMeasurementHashAlgo::TPM_ALG_SHA_384;
        requester.common.peer_info.peer_cert_chain.cert_chain = REQ_CERT_CHAIN_DATA;
        requester.common.reset_runtime_info();

        let measurement_operation = SpdmMeasurementOperation::SpdmMeasurementQueryTotalNumber;
        let mut total_number: u8 = 0;
        let status = requester
            .send_receive_spdm_measurement(
                None,
                SpdmMeasurementeAttributes::SIGNATURE_REQUESTED,
                measurement_operation,
                &mut total_number,
                0,
            )
            .is_ok();
        assert!(status);

        let measurement_operation = SpdmMeasurementOperation::SpdmMeasurementRequestAll;
        let status = requester
            .send_receive_spdm_measurement(
                None,
                SpdmMeasurementeAttributes::SIGNATURE_REQUESTED,
                measurement_operation,
                &mut total_number,
                0,
            )
            .is_ok();
        assert!(status);

        let measurement_operation = SpdmMeasurementOperation::Unknown(5);
        let status = requester
            .send_receive_spdm_measurement(
                None,
                SpdmMeasurementeAttributes::SIGNATURE_REQUESTED,
                measurement_operation,
                &mut total_number,
                0,
            )
            .is_ok();
        assert!(status);
    }
}
