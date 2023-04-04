// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

type ModuleId = (Address, Identifier);

// e.g. 0x1::account::Account::sequence_number
type FieldId = (Address, ModuleName, Identifier, Identifier);

// e.g. 0x1::account::Account::[coin_register_events,guid,id,addr]
type AccessPath = (Address, Identifier, Identifier, Vec<Identifier>);

struct TypeAccessor {
    module_id: ModuleId,
    field_info: HashMap<FieldId, MoveType>,
}

impl TypeAccessor {
    // This is a function that recursively builds up a map of field types.
    // We assume here that the user only cares about the types of fields.
    async fn build(module_id: ModuleId) -> Self {
        let mut field_info = HashMap::new();

        let mut modules_to_resolve = vec![module_id];
        let mut modules_seen = HashSet::new();

        // Start with the top level module.
        while let Some(module_id) = modules.pop() {
            if modules_seen.contains(module_id) {
                continue;
            }
            modules_seen.insert(module_id);

            let module: MoveModule = aptos_client.get_module(address, name).await;
            let (address, name) = module_id;

            // For each struct in the module look through the types of the fields and
            // determine any more modules we need to look up.
            for struc in module.structs {
                let mut types_to_resolve = Vec::new();
                let mut types_seen = HashSet::new();

                for field in struc.fields {
                    types_to_resolve.push(field.typ);
                    field_info.insert((address, name, struc.name, field.name), field.typ);
                }

                // Go through the types recursively, adding more modules to
                // `modules_to_resolve`.
                while let Some(typ) = types_to_resolve.pop() {
                    if types_seen.contains(typ) {
                        continue;
                    }
                    types_seen.insert(typ);

                    // For types that refer to other types, add those to the list of
                    // types. This continues until we hit leaves / a cycle.
                    match typ {
                        MoveType::Vector { typ } => {
                            types_to_resolve.push(typ);
                        },
                        MoveType::Reference { _mutable, typ } => {
                            types_to_resolve.push(typ);
                        },
                        MoveType::Struct(struct_tag) => {
                            modules_to_resolve.push((struct_tag.address, struct_tag.module));
                        },
                        other => {},
                    }
                }
            }
        }

        Self {
            module_id,
            field_info,
        }
    }

    fn get_type(access_path: AccessPath) -> MoveType {
        todo!();
    }
}
