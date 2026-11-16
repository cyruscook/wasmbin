use wasmbin::Module;
use wasmbin::io::Encode;
use wasmbin::sections::{Import, ImportDesc, ImportGroup, ImportPath, payload};
use wasmbin::visit::Visit;

fn module_bytes(payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"\0asm\x01\0\0\0\x02".to_vec();
    payload.encode(&mut bytes).unwrap();
    bytes
}

fn decode_payload(payload: &[u8]) -> Result<Module, wasmbin::io::DecodeError> {
    let mut module = Module::decode_from(module_bytes(payload).as_slice())?;
    module.visit_mut(|()| {})?;
    Ok(module)
}

fn single(module: &str, name: &str, desc: ImportDesc) -> ImportGroup {
    Import {
        path: ImportPath {
            module: module.into(),
            name: name.into(),
        },
        desc,
    }
    .into()
}

#[test]
fn ordinary_imports_and_module_access() {
    let bytes = b"\x02\0\0\0\0\x01m\x01f\0\x01";
    let mut module = decode_payload(bytes).unwrap();
    let groups = module
        .find_std_section_mut::<payload::Import>()
        .unwrap()
        .try_contents_mut()
        .unwrap();
    assert_eq!(
        *groups,
        vec![
            single("", "", ImportDesc::Func(0.into())),
            single("m", "f", ImportDesc::Func(1.into()))
        ]
    );
    assert_eq!(groups[0].module(), "");
    *groups[0].module_mut() = "env".into();
    let output = module.encode_into(Vec::new()).unwrap();
    let mut decoded = Module::decode_from(output.as_slice()).unwrap();
    decoded.visit_mut(|()| {}).unwrap();
    assert_eq!(module, decoded);
}

#[cfg(not(feature = "compact-import-section"))]
#[test]
fn compact_encodings_require_feature() {
    for payload in [b"\x01\x01m\0\x7f\0".as_slice(), b"\x01\x01m\0\x7e\0\0\0"] {
        // Loading a lazy section remains valid, but accessing it must reject the marker.
        let module = Module::decode_from(module_bytes(payload).as_slice()).unwrap();
        assert!(
            module
                .find_std_section::<payload::Import>()
                .unwrap()
                .try_contents()
                .is_err()
        );
        assert!(decode_payload(payload).is_err());
    }
}

#[cfg(feature = "compact-import-section")]
mod enabled {
    use super::*;
    use wasmbin::indices::TypeId;
    use wasmbin::sections::ImportItem;
    use wasmbin::types::{
        AbstractHeapType, ExceptionType, GlobalType, Limits, MemType, RefType, TableType, ValueType,
    };

    fn limits() -> Limits {
        Limits {
            min: 1,
            max: None,
            is_64: false,
            #[cfg(feature = "threads")]
            is_shared: false,
            #[cfg(feature = "custom-page-sizes")]
            page_size: None,
        }
    }

    fn compact(items: Vec<ImportItem>) -> ImportGroup {
        ImportGroup::Compact {
            module: "m".into(),
            items,
        }
    }

    fn shared(desc: ImportDesc, names: &[&str]) -> ImportGroup {
        ImportGroup::CompactSameType {
            module: "m".into(),
            desc,
            names: names.iter().map(|s| (*s).into()).collect(),
        }
    }

    fn assert_fixture(bytes: &[u8], expected: Vec<ImportGroup>) {
        let module = decode_payload(bytes).unwrap();
        assert_eq!(
            module
                .find_std_section::<payload::Import>()
                .unwrap()
                .try_contents()
                .unwrap(),
            &expected
        );
        // decode_payload mutably visits the section, preventing raw-byte passthrough.
        assert_eq!(module.encode_into(Vec::new()).unwrap(), module_bytes(bytes));
        let mut encoded = Vec::new();
        expected.encode(&mut encoded).unwrap();
        assert_eq!(encoded, bytes);
    }

    #[test]
    fn mixed_groups_preserve_forms_order_and_empty_groups() {
        assert_fixture(
            b"\x05\x01m\x01s\0\x02\x01m\0\x7f\0\x01m\0\x7e\0\x03\0\x01m\0\x7f\x02\0\0\x04\x01x\0\x05\x01m\0\x7e\0\x06\x03\x01x\x01x\x02\xc3\xa9",
            vec![single("m", "s", ImportDesc::Func(2.into())), compact(vec![]), shared(ImportDesc::Func(3.into()), &[]), compact(vec![ImportItem { name: "".into(), desc: ImportDesc::Func(4.into()) }, ImportItem { name: "x".into(), desc: ImportDesc::Func(5.into()) }]), shared(ImportDesc::Func(6.into()), &["x", "x", "é"])],
        );
    }

    #[test]
    fn every_descriptor_in_both_compact_forms() {
        let cases = [
            (b"\0\x81\x01".as_slice(), ImportDesc::Func(129.into())),
            (
                b"\x01\x70\0\x01",
                ImportDesc::Table(TableType {
                    elem_type: RefType::AbstractHeapType(AbstractHeapType::Func),
                    limits: limits(),
                }),
            ),
            (b"\x02\0\x01", ImportDesc::Mem(MemType { limits: limits() })),
            (
                b"\x03\x7f\x01",
                ImportDesc::Global(GlobalType {
                    value_type: ValueType::I32,
                    mutable: true,
                }),
            ),
            (
                b"\x04\0\x02",
                ImportDesc::Exception(ExceptionType {
                    func_type: 2.into(),
                }),
            ),
        ];
        for (desc_bytes, desc) in cases {
            let mut bytes = b"\x01\x01m\0\x7f\x01\x01x".to_vec();
            bytes.extend_from_slice(desc_bytes);
            assert_fixture(
                &bytes,
                vec![compact(vec![ImportItem {
                    name: "x".into(),
                    desc: desc.clone(),
                }])],
            );
            let mut bytes = b"\x01\x01m\0\x7e".to_vec();
            bytes.extend_from_slice(desc_bytes);
            bytes.extend_from_slice(b"\x01\x01x");
            assert_fixture(&bytes, vec![shared(desc, &["x"])]);
        }
    }

    #[test]
    fn multibyte_counts() {
        for marker in [0x7f, 0x7e] {
            let mut bytes = b"\x01\x01m\0".to_vec();
            bytes.push(marker);
            if marker == 0x7e {
                bytes.extend_from_slice(b"\0\0");
            }
            bytes.extend_from_slice(b"\x80\x01"); // 128 items
            for _ in 0..128 {
                bytes.extend_from_slice(b"\x01x");
                if marker == 0x7f {
                    bytes.extend_from_slice(b"\0\0");
                }
            }
            let group = if marker == 0x7f {
                compact(vec![
                    ImportItem {
                        name: "x".into(),
                        desc: ImportDesc::Func(0.into())
                    };
                    128
                ])
            } else {
                shared(ImportDesc::Func(0.into()), &vec!["x"; 128])
            };
            assert_fixture(&bytes, vec![group]);
        }
        let mut bytes = b"\x80\x01".to_vec();
        for _ in 0..128 {
            bytes.extend_from_slice(b"\x01m\0\x7f\0");
        }
        assert_fixture(&bytes, vec![compact(vec![]); 128]);
    }

    #[test]
    fn padded_lengths_canonicalize() {
        for (padded, canonical) in [
            (
                b"\x81\0\x81\0m\x80\x80\x80\x80\0\x7f\x81\0\x81\0x\0\0".as_slice(),
                b"\x01\x01m\0\x7f\x01\x01x\0\0".as_slice(),
            ),
            (
                b"\x01\x01m\x80\0\x7e\0\0\x81\0\x81\0x",
                b"\x01\x01m\0\x7e\0\0\x01\x01x",
            ),
        ] {
            let module = decode_payload(padded).unwrap();
            assert_eq!(module, decode_payload(canonical).unwrap());
            assert_eq!(
                module.encode_into(Vec::new()).unwrap(),
                module_bytes(canonical)
            );
        }
    }

    #[test]
    fn malformed_payloads_and_truncations() {
        let invalid: &[&[u8]] = &[
            b"\x01\x01m\x01x\x7f\0",
            b"\x01\x01m\x01x\x7e\0\0\0", // nonempty marker names
            b"\x01\0\0\x7d",
            b"\x01\0\0\xff\0",
            b"\x01\0\0\xfe\0", // invalid markers
            b"\x01\0\0\x7f\x01\0\x7f\0",
            b"\x01\0\0\x7e\x7e\0", // nested groups
            b"\x01\0\0\x7f\x01\0\x05",
            b"\x01\0\0\x7e\x05\0", // invalid descriptors
            b"\x01\x01\xff\0\x7f\0",
            b"\x01\0\0\x7f\x01\x01\xff\0\0",
            b"\x01\0\0\x7e\0\0\x01\x01\xff", // UTF-8
            b"\x01\0\0\x7f\xff\xff\xff\xff\x10",
            b"\x01\0\0\x7e\0\0\xff\xff\xff\xff\x10", // count overflow
            b"\x01\0\0\x7f\x01\xff\xff\xff\xff\x10", // name length overflow
            b"\x01\0\0\x7f\0\0",
            b"\x01\0\0\x7e\0\0\0\0", // trailing data
        ];
        for bytes in invalid {
            assert!(decode_payload(bytes).is_err(), "accepted {bytes:02x?}");
        }
        for complete in [
            b"\x01\x01m\0\x7f\x02\x01a\0\0\x01b\0\x01".as_slice(),
            b"\x01\x01m\0\x7e\0\0\x02\x01a\x01b",
        ] {
            for len in 0..complete.len() {
                assert!(
                    decode_payload(&complete[..len]).is_err(),
                    "accepted prefix {len}"
                );
            }
        }
        let err = decode_payload(b"\x01\0\0\x7f\x01\0\x05")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("[0]:<ImportGroup::Compact>.items[0]:<ImportItem>.desc"),
            "{err}"
        );
        let err = decode_payload(b"\x01\0\0\x7e\0\0\x01\x01\xff")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ImportGroup::CompactSameType>.names[0]"),
            "{err}"
        );
    }

    #[test]
    fn visitors_mutate_stored_descriptors_and_names() {
        let mut module = decode_payload(
            b"\x03\x01m\x01s\0\0\x01m\0\x7f\x01\x01x\0\x01\x01m\0\x7e\0\x02\x02\x01a\x01b",
        )
        .unwrap();
        let mut indices = Vec::new();
        module.visit(|id: &TypeId| indices.push(id.index)).unwrap();
        assert_eq!(indices, [0, 1, 2]);
        module.visit_mut(|id: &mut TypeId| id.index += 10).unwrap();
        module
            .visit_mut(|name: &mut String| name.push('!'))
            .unwrap();
        let groups = module
            .find_std_section_mut::<payload::Import>()
            .unwrap()
            .try_contents_mut()
            .unwrap();
        for group in groups {
            assert_eq!(group.module(), "m!");
            *group.module_mut() = "env".into();
        }
        let encoded = module.encode_into(Vec::new()).unwrap();
        let mut decoded = Module::decode_from(encoded.as_slice()).unwrap();
        decoded.visit_mut(|()| {}).unwrap();
        assert_eq!(decoded, module);
        assert_eq!(
            decoded
                .find_std_section::<payload::Import>()
                .unwrap()
                .try_contents()
                .unwrap(),
            &vec![
                single("env", "s!", ImportDesc::Func(10.into())),
                ImportGroup::Compact {
                    module: "env".into(),
                    items: vec![ImportItem {
                        name: "x!".into(),
                        desc: ImportDesc::Func(11.into())
                    }]
                },
                ImportGroup::CompactSameType {
                    module: "env".into(),
                    desc: ImportDesc::Func(12.into()),
                    names: vec!["a!".into(), "b!".into()]
                }
            ]
        );
    }
}
