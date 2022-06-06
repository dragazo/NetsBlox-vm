use std::prelude::v1::*;
use std::collections::VecDeque;
use std::rc::Rc;
use std::iter;

use netsblox_ast as ast;
use slotmap::SlotMap;

use crate::bytecode::*;
use crate::runtime::*;
use crate::process::*;

slotmap::new_key_type! {
    struct EntityKey;
    struct ProcessKey;
}

struct Script {
    hat: Option<ast::Hat>,
    process: Process,
    start_pos: usize,
    context_queue: VecDeque<SymbolTable>,
}
impl Script {
    fn consume_context(&mut self) {
        if self.process.state != ProcessState::Running {
            if let Some(context) = self.context_queue.pop_front() {
                self.process.initialize(self.start_pos, context);
            }
        }
    }
    fn schedule(&mut self, max_queue: usize, context: SymbolTable) {
        self.context_queue.push_back(context);
        self.consume_context();
        if self.context_queue.len() > max_queue {
            self.context_queue.pop_back();
        }
    }
    fn step<Clock>(&mut self, global_context: &mut GlobalContext<Clock>, entity_context: &mut EntityContext) -> StepType {
        unimplemented!()
    }
}

// -----------------------------------------------------------------

#[derive(PartialEq, Eq)]
enum EntityKind {
    Stage,
    Original,
    Clone,
}
struct EntityContext {
    fields: SymbolTable,
    kind: EntityKind,
}
struct Entity {
    context: EntityContext,
    scripts: Vec<Script>,
    script_queue_pos: usize,
}
impl Entity {
    fn step<Clock>(&mut self, global_context: &mut GlobalContext<Clock>) -> StepType {
        if self.scripts.is_empty() { return StepType::Yield }
        let res = self.scripts[self.script_queue_pos].step(global_context, &mut self.context);
        match res {
            StepType::Normal => (), // keep executing same script
            StepType::Yield => self.script_queue_pos = (self.script_queue_pos + 1) % self.scripts.len(), // yield to next script
        }
        res
    }
}

// -----------------------------------------------------------------

pub enum UserInput {
    ClickStart,
}
struct GlobalContext {
    ref_pool: RefPool,
    globals: SymbolTable,
}
pub struct Project {
    context: GlobalContext,
    entities: SlotMap<EntityKey, Entity>,
    entity_queue: VecDeque<EntityKey>,
    processes: SlotMap<ProcessKey, Process>,
    process_queue: VecDeque<ProcessKey>,
    max_call_depth: usize,
}
impl Project {
    pub fn new(role: &ast::Role, max_call_depth: usize) -> Self {
        let mut ref_pool = RefPool::new();
        let mut globals = SymbolTable::default();
        for glob in role.globals.iter() {
            globals.define(glob.trans_name.clone(), Value::from_ast(&glob.value, &mut ref_pool));
        }

        let (code, locations) = ByteCode::compile(role);
        let code = Rc::new(code);

        let mut entities: SlotMap<EntityKey, _> = Default::default();
        let mut entity_queue = VecDeque::with_capacity(role.sprites.len());
        for (i, (entity, locs)) in iter::zip(&role.sprites, &locations.entities).enumerate() {
            let mut fields = SymbolTable::default();
            for field in entity.fields.iter() {
                fields.define(field.trans_name.clone(), Value::from_ast(&field.value, &mut ref_pool));
            }

            let mut scripts = Vec::with_capacity(entity.scripts.len());
            for (script, loc) in iter::zip(&entity.scripts, &locs.scripts) {
                scripts.push(Script {
                    hat: script.hat.clone(),
                    process: Process::new(code.clone(), max_call_depth),
                    start_pos: *loc,
                    context_queue: Default::default(),
                })
            }

            entity_queue.push_back(entities.insert(Entity {
                context: EntityContext {
                    fields,
                    kind: if i == 0 { EntityKind::Stage } else { EntityKind::Original },
                },
                scripts,
                script_queue_pos: 0
            }));
        }

        Self {
            context: GlobalContext { globals, ref_pool },
            entities, entity_queue, max_call_depth,
        }
    }
    pub fn input(&mut self, input: UserInput) {
        match input {
            UserInput::ClickStart => {
                for (_, entity) in self.entities.iter_mut() {
                    for script in entity.scripts.iter_mut() {
                        if let Some(ast::Hat::OnFlag { .. }) = &script.hat {
                            script.schedule(0, Default::default());
                        }
                    }
                }
            }
        }
    }
    pub fn step(&mut self) -> StepType {
        let (key, entity) = loop {
            match self.entity_queue.pop_front() {
                None => return,
                Some(key) => match self.entities.get_mut(key) {
                    None => (), // prune invalid key due to pop
                    Some(entity) => break (key, entity),
                },
            }
        };

        match entity.step(&mut self.context) {
            StepType::Normal => self.entity_queue.push_front(key), // keep executing same entity
            StepType::Yield => self.entity_queue.push_back(key), // yield to next entity
        }
    }
}