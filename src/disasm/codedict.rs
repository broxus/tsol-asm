/*
 * Copyright 2023 EVERX DEV SOLUTIONS LTD.
 *
 * Licensed under the SOFTWARE EVALUATION License (the "License"); you may not use
 * this file except in compliance with the License.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific EVERX DEV software governing permissions and
 * limitations under the License.
 */

use super::Result;
use super::{
    loader::Loader,
    types::{Code, Instruction, InstructionParameter},
};
use crate::fail;
use std::collections::HashMap;
use tycho_types::cell::CellFamily;
use tycho_types::dict::{dict_find_bound_owned, dict_find_owned, DictBound};
use tycho_types::prelude::{Cell, CellSlice};

fn match_dictpushconst_dictugetjmp(
    pair: &mut [Instruction],
) -> Option<&mut Vec<InstructionParameter>> {
    let insn2 = pair.get(1)?.name();
    if insn2 != "DICTUGETJMP" && insn2 != "DICTUGETJMPZ" {
        return None;
    }
    let insn1 = pair.get_mut(0)?;
    if insn1.name() != "DICTPUSHCONST" && insn1.name() != "PFXDICTSWITCH" {
        return None;
    }
    Some(insn1.params_mut())
}

impl Code {
    fn process_dictpushconst_dictugetjmp(code: &mut Code) {
        for pair in code.chunks_mut(2) {
            if let Some(params) = match_dictpushconst_dictugetjmp(pair) {
                // TODO transform cell to code right here (for nested dicts)
                params.push(InstructionParameter::CodeDictMarker)
            }
        }
    }

    fn traverse_code_tree(&mut self, process: fn(&mut Code)) {
        let mut stack = vec![self];
        while let Some(code) = stack.pop() {
            process(code);
            for insn in code.iter_mut() {
                for param in insn.params_mut() {
                    if let InstructionParameter::Code {
                        code: ref mut inner,
                        cell: _,
                    } = param
                    {
                        stack.push(inner)
                    }
                }
            }
        }
    }

    pub fn elaborate_dictpushconst_dictugetjmp(&mut self) {
        self.traverse_code_tree(Self::process_dictpushconst_dictugetjmp)
    }
}

pub(super) struct DelimitedHashmapE {
    dict: Cell,
    key_size: usize,
    map: HashMap<Vec<u8>, (u64, usize, Code)>,
}

impl DelimitedHashmapE {
    pub fn new(cell: Cell, key_size: usize) -> Self {
        Self {
            dict: cell,
            key_size,
            map: HashMap::new(),
        }
    }
    fn slice_eq_data(lhs: &CellSlice, rhs: &CellSlice) -> bool {
        lhs.lex_cmp(rhs).unwrap() == std::cmp::Ordering::Equal
    }
    fn slice_eq_children(lhs: &CellSlice, rhs: &CellSlice) -> bool {
        let refs_count = lhs.size_refs();
        if refs_count != rhs.size_refs() {
            return false;
        }
        for i in 0..refs_count {
            let ref1 = lhs.get_reference_cloned(i).unwrap();
            let ref2 = rhs.get_reference_cloned(i).unwrap();
            if ref1.repr_hash() != ref2.repr_hash() {
                return false;
            }
        }
        true
    }
    fn locate(mut slice: CellSlice, target: &CellSlice, path: Vec<u8>) -> Result<(Vec<u8>, usize)> {
        if Self::slice_eq_children(&slice, target) {
            loop {
                if Self::slice_eq_data(&slice, target) {
                    return Ok((path, slice.range().offset_bits() as usize));
                }
                if slice.load_bit().is_err() {
                    break;
                }
            }
        }
        for i in 0..slice.size_refs() {
            let child_cell = slice.get_reference_cloned(i)?;
            let child = child_cell.as_slice()?;
            let mut next = path.clone();
            next.push(i as u8);
            if let Ok(v) = Self::locate(child, target, next) {
                return Ok(v);
            }
        }
        fail!("not found")
    }
    pub fn mark(&mut self) -> Result<()> {
        let dict_slice = self.dict.as_slice()?;

        if let Ok(Some((mut key, mut slice))) = dict_find_bound_owned(
            Some(&self.dict),
            self.key_size as u16,
            DictBound::Min,
            false,
            Cell::empty_context(),
        ) {
            loop {
                let mut key_slice = key.as_data_slice();
                let id = key_slice.load_uint(self.key_size as u16)?;
                let mut value = CellSlice::apply(&slice)?;
                let loc = Self::locate(dict_slice.clone(), &value, vec![])?;
                let mut loader = Loader::new(false);
                let code = loader.load(&mut value, true)?;
                if self.map.insert(loc.0, (id, loc.1, code)).is_some() {
                    fail!("non-unique path found")
                }

                let next = dict_find_owned(
                    Some(&self.dict),
                    self.key_size as u16,
                    key.as_data_slice(),
                    DictBound::Max,
                    false,
                    false,
                    Cell::empty_context(),
                )?;
                match next {
                    None => {
                        break;
                    }
                    Some((new_key, new_slice)) => {
                        key = new_key;
                        slice = new_slice;
                    }
                }
            }
        }

        Ok(())
    }
    fn print_impl(&self, cell: &Cell, indent: &str, path: Vec<u8>) -> String {
        let mut text = String::new();
        text += &format!("{}.cell ", indent);
        text += &format!("{{ ;; #{}\n", hex::encode(cell.repr_hash().0));
        let inner_indent = String::from("  ") + indent;
        let mut slice = cell.as_slice().unwrap();
        if let Some((id, offset, code)) = self.map.get(&path) {
            let aux = slice.load_prefix(*offset as u16, 0).unwrap();
            text += &format!("{}.blob x{}\n", inner_indent, aux.display_data());
            text += &format!("{};; method {}\n", inner_indent, id);
            text += &code.print(&inner_indent, true, 0);
        } else {
            if slice.size_bits() > 0 {
                text += &format!("{}.blob x{}\n", inner_indent, slice.display_data());
            }
            for i in 0..cell.reference_count() {
                let mut path = path.clone();
                path.push(i as u8);
                text += &self.print_impl(
                    &cell.reference_cloned(i).unwrap(),
                    inner_indent.as_str(),
                    path,
                );
            }
        }
        text += &format!("{}}}\n", indent);
        text
    }
    pub fn print(&self, indent: &str) -> String {
        self.print_impl(&self.dict, indent, vec![])
    }
}
