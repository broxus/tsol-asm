/*
* Copyright (C) 2019-2023 EverX. All Rights Reserved.
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

use crate::debug::DbgNode;
use crate::{CompileResult, DbgInfo, OperationError};
use tycho_types::prelude::CellBuilder;
use tycho_vm::OwnedCellSlice;

#[derive(Clone, Default)]
pub struct Unit {
    builder: CellBuilder,
    dbg: DbgNode,
}

impl Unit {
    pub fn new(builder: CellBuilder, dbg: DbgNode) -> Self {
        Self { builder, dbg }
    }
    pub fn finalize(self) -> (OwnedCellSlice, DbgInfo) {
        let cell = self.builder.build().unwrap();
        let slice = OwnedCellSlice::new_allow_exotic(cell.clone()).clone();
        let dbg_info = DbgInfo::from(cell, self.dbg);
        (slice, dbg_info)
    }
}

pub struct Units {
    units: Vec<Unit>,
}

impl Default for Units {
    fn default() -> Self {
        Self::new()
    }
}

impl Units {
    /// Constructor
    pub fn new() -> Self {
        Self {
            units: vec![Unit::default()],
        }
    }
    /// Writes assembled unit
    pub fn write_unit(&mut self, unit: Unit) -> CompileResult {
        self.units.push(unit);
        Ok(())
    }
    /// Writes simple command
    pub fn write_command(&mut self, command: &[u8], dbg: DbgNode) -> CompileResult {
        self.write_command_bitstring(command, command.len() * 8, dbg)
    }
    pub fn write_command_bitstring(
        &mut self,
        command: &[u8],
        bits: usize,
        dbg: DbgNode,
    ) -> CompileResult {
        if let Some(last) = self.units.last_mut() {
            let orig_offset = last.builder.size_bits();
            if last.builder.store_raw(command, bits as u16).is_ok() {
                last.dbg.inline_node(orig_offset as usize, dbg);
                return Ok(());
            }
        }
        if let Ok(new_last) = CellBuilder::from_raw_data(command, bits as u16) {
            self.units.push(Unit::new(new_last, dbg));
            return Ok(());
        }
        Err(OperationError::NotFitInSlice)
    }
    /// Writes command with additional references
    pub fn write_composite_command(
        &mut self,
        command: &[u8],
        references: Vec<CellBuilder>,
        dbg: DbgNode,
    ) -> CompileResult {
        assert_eq!(references.len(), dbg.children.len());
        if let Some(mut last) = self.units.last().cloned() {
            let orig_offset = last.builder.size_bits();
            if last.builder.spare_capacity_refs() > references.len() as u8 // one cell remains reserved for finalization
                && last.builder.store_raw(command, (command.len() * 8) as u16).is_ok()
                && checked_append_references(&mut last.builder, &references)?
            {
                last.dbg.inline_node(orig_offset as usize, dbg);
                *self.units.last_mut().unwrap() = last;
                return Ok(());
            }
        }
        let mut new_last = CellBuilder::new();
        if new_last
            .store_raw(command, (command.len() * 8) as u16)
            .is_ok()
            && checked_append_references(&mut new_last, &references)?
        {
            self.units.push(Unit::new(new_last, dbg));
            return Ok(());
        }
        Err(OperationError::NotFitInSlice)
    }

    /// Puts recorded cells in a linear sequence
    pub fn finalize(mut self) -> (CellBuilder, DbgNode) {
        let mut cursor = self.units.pop().expect("cells can't be empty");
        while let Some(mut destination) = self.units.pop() {
            let orig_offset = destination.builder.size_bits();
            let cell = cursor.builder.build().unwrap();
            let slice = cell.as_slice().unwrap();
            // try to inline cursor into destination
            if destination.builder.store_slice(slice).is_ok() {
                destination
                    .dbg
                    .inline_node(orig_offset as usize, cursor.dbg);
            } else {
                // otherwise just attach cursor to destination as a reference
                destination.builder.store_reference(cell).unwrap();
                destination.dbg.append_node(cursor.dbg);
            }
            cursor = destination;
        }
        (cursor.builder, cursor.dbg)
    }
}

fn checked_append_references(
    builder: &mut CellBuilder,
    refs: &[CellBuilder],
) -> Result<bool, OperationError> {
    for reference in refs {
        let cloned_builder = reference.clone();
        let cell_result = cloned_builder.build();
        let cell = cell_result.unwrap();
        if builder.store_reference(cell).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}
