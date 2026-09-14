use super::*;
use cadder_ipc::{OperationAccess, OperationDeadlineClass, OperationShape, message_types};

#[test]
fn operation_fence_covers_only_retained_non_shutdown_mutations() {
  let fenced = OPERATION_REGISTRY
    .iter()
    .filter(|definition| operation_uses_fence(definition))
    .map(|definition| definition.name())
    .collect::<Vec<_>>();

  assert_eq!(
    fenced,
    vec![
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      message_types::SET_DOMAIN_ENABLED_REQUEST,
    ]
  );
}

#[test]
fn every_retained_operation_is_unary_with_a_bounded_deadline() {
  assert!(OPERATION_REGISTRY.iter().all(|definition| {
    definition.shape() == OperationShape::Unary
      && matches!(
        definition.deadline(),
        OperationDeadlineClass::Ordinary
          | OperationDeadlineClass::Reload
          | OperationDeadlineClass::Shutdown
      )
  }));
}

#[test]
fn owned_mutation_worker_matches_retained_state_mutations() {
  for operation in OPERATION_REGISTRY.iter() {
    let expected = operation.access() == OperationAccess::Mutation
      && operation.name() != message_types::SHUTDOWN_DAEMON_REQUEST;
    assert_eq!(
      owned_mutation_uses_worker(operation.name()),
      expected,
      "{}",
      operation.name()
    );
  }
}
