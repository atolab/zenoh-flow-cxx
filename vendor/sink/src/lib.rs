//
// Copyright (c) 2017, 2021 ADLINK Technology Inc.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// http://www.eclipse.org/legal/epl-2.0, or the Apache License, Version 2.0
// which is available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//
// Contributors:
//   ADLINK zenoh team, <zenoh@adlink-labs.tech>
//

use async_trait::async_trait;
use cxx::UniquePtr;
use std::{fmt::Debug, sync::Arc};
use zenoh_flow::{
    runtime::message::DataMessage, Configuration, Context, Node, Sink, State, ZFError, ZFResult,
    ZFState, runtime::deadline::E2EDeadlineMiss
};

extern crate zenoh_flow;

#[cxx::bridge(namespace = "zenoh::flow")]
pub mod ffi {
    pub struct Context {
        pub mode: usize,
    }

    pub struct Configuration {
        pub key: String,
        pub value: String,
    }

    pub struct Input {
        pub data: Vec<u8>,
        pub timestamp: u64,
        pub e2d_deadline_miss: Vec<E2EDeadlineMiss>,
    }

    pub struct E2EDeadlineMiss {
        pub from: FromDescriptor,
        pub to: ToDescriptor,
        pub start: u64,
        pub end: u64,
    }

    pub struct FromDescriptor {
        pub node: String,
        pub output: String,
    }

    pub struct ToDescriptor {
        pub node: String,
        pub input: String,
    }

    unsafe extern "C++" {
        include!("sink.hpp");

        type State;

        fn initialize(configuration: &Vec<Configuration>) -> UniquePtr<State>;

        fn run(context: &mut Context, state: &mut UniquePtr<State>, input: Input) -> Result<()>;
    }
}

/*
 *
 * Zenoh Flow glue.
 *
 */

unsafe impl Send for ffi::State {}
unsafe impl Sync for ffi::State {}

pub struct StateWrapper {
    pub state: UniquePtr<ffi::State>,
}

impl ZFState for StateWrapper {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Debug for StateWrapper {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl From<&mut zenoh_flow::Context> for ffi::Context {
    fn from(context: &mut zenoh_flow::Context) -> Self {
        Self { mode: context.mode }
    }
}

impl ffi::Input {
    fn from_data_message(
        data_message: &mut zenoh_flow::runtime::message::DataMessage,
    ) -> ZFResult<Self> {
        let data = data_message.get_inner_data().try_as_bytes()?.as_ref().clone();
        let e2d_deadline_miss: Vec<ffi::E2EDeadlineMiss> = data_message
        .get_missed_end_to_end_deadlines()
        .iter()
        .map(|e2e_deadline| e2e_deadline.into())
        .collect();

        Ok(Self {
            data,
            timestamp: data_message.get_timestamp().get_time().as_u64(),
            e2d_deadline_miss,
        })
    }
}

impl From<&E2EDeadlineMiss> for ffi::E2EDeadlineMiss {
    fn from(e2d_deadline_miss: &E2EDeadlineMiss) -> Self {
        let to = ffi::ToDescriptor {
            node: e2d_deadline_miss.to.node.as_ref().clone().into(),
            input: e2d_deadline_miss.to.input.as_ref().clone().into(),
        };
        let from = ffi::FromDescriptor {
            node: e2d_deadline_miss.from.node.as_ref().clone().into(),
            output: e2d_deadline_miss.from.output.as_ref().clone().into(),
        };

        Self {
            from,
            to,
            start: e2d_deadline_miss.start.get_time().as_u64(),
            end: e2d_deadline_miss.end.get_time().as_u64(),
        }
    }
}

/*
 *
 * CxxSink implementation.
 *
 */

pub struct CxxSink;

impl Node for CxxSink {
    fn initialize(&self, configuration: &Option<Configuration>) -> ZFResult<State> {
        let cxx_configuration = match configuration {
            Some(config) => match config.as_object() {
                Some(config) => {
                    let mut conf = vec![];
                    for (key, value) in config {
                        let entry = ffi::Configuration {
                            key: key.clone(),
                            value: value
                                .as_str()
                                .ok_or_else(|| ZFError::GenericError)?
                                .to_string(),
                        };
                        conf.push(entry);
                    }
                    conf
                }
                None => vec![],
            },

            None => vec![],
        };

        let state = {
            #[allow(unused_unsafe)]
            unsafe {
                ffi::initialize(&cxx_configuration)
            }
        };
        Ok(State::from(StateWrapper { state }))
    }

    fn finalize(&self, _state: &mut State) -> ZFResult<()> {
        Ok(())
    }
}

#[async_trait]
impl Sink for CxxSink {
    async fn run(
        &self,
        context: &mut Context,
        dyn_state: &mut State,
        mut input: DataMessage,
    ) -> ZFResult<()> {
        let mut cxx_context = ffi::Context::from(context);
        let wrapper = dyn_state.try_get::<StateWrapper>()?;
        let cxx_input = ffi::Input::from_data_message(&mut input)?;

        {
            #[allow(unused_unsafe)]
            unsafe {
                Ok(ffi::run(&mut cxx_context, &mut wrapper.state, cxx_input)
                    .map_err(|_| ZFError::GenericError)?)
            }
        }
    }
}

zenoh_flow::export_sink!(register);

fn register() -> ZFResult<Arc<dyn Sink>> {
    Ok(Arc::new(CxxSink) as Arc<dyn Sink>)
}
