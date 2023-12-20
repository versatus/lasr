use async_trait::async_trait;
use ractor::{MessagingErr, Message, concurrency::oneshot, RpcReplyPort};
use crate::{
    EoMessage, 
    SchedulerMessage,
    DaClientMessage,
    EngineMessage,
    ValidatorMessage,
    RpcMessage,
    RegistryMessage,
    RegistryResponse, ActorType
};
use ractor::ActorRef;

pub trait TheatreMember {
    type Msg: Message;
    fn cast(&self, msg: Self::Msg) -> Result<(), MessagingErr<Self::Msg>>;
}

impl TheatreMember for ActorRef<EoMessage> {
    type Msg = EoMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<SchedulerMessage> {
    type Msg = SchedulerMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<EngineMessage> {
    type Msg = EngineMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<ValidatorMessage> {
    type Msg = ValidatorMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<DaClientMessage> {
    type Msg = DaClientMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<RpcMessage> {
    type Msg = RpcMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<RegistryMessage> {
    type Msg = RegistryMessage;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

impl TheatreMember for ActorRef<RegistryResponse> {
    type Msg = RegistryResponse;
    fn cast(&self, msg: Self::Msg) -> Result<(), ractor::MessagingErr<Self::Msg>> {
       self.cast(msg) 
    }
}

#[async_trait]
pub trait RegistryMember {
    type Err: std::error::Error + Default;

    async fn get_actor<T, F, M>(
        &self, 
        handler: F,
        actor_type: ActorType,
        registry: ActorRef<RegistryMessage>,
    ) -> Result<M, Self::Err> 
    where 
        T: Message,
        F: FnOnce(RegistryResponse) -> Result<M, Box<dyn std::error::Error>> + Send 
    {
        let (tx, rx) = oneshot();
        let reply = RpcReplyPort::from(tx);
        let msg = RegistryMessage::GetActor(actor_type, reply);
        registry.cast(msg).map_err(|_| {
            Self::Err::default()
        })?;

        crate::actors::handle_actor_response(rx, handler).await.map_err(|_| {
            Self::Err::default()
        })
    }

    async fn send_to_actor<T, F, A, M>(
        &self,
        handler: F,
        actor_type: ActorType,
        message: T, 
        registry: ActorRef<RegistryMessage>
    ) -> Result<(), Self::Err> 
    where
        T: Message,
        F: FnOnce(RegistryResponse) -> Result<A, Box<dyn std::error::Error>> + Send,
        A: Into<Option<M>>,
        M: TheatreMember<Msg = T>
    {
        let actor = self.get_actor::<T, _, A>(
            handler,
            actor_type,
            registry
        ).await.map_err(|_| {
            Self::Err::default()
        })?.into().ok_or(
            Self::Err::default()
        )?;
        
        actor.cast(
            message
        ).map_err(|_| {
            Self::Err::default()
        })?;

        Ok(())
    }
}

