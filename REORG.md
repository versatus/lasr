/src

4 libraries

# binary libs
lasr_cli
lasr_node

# libraries
lasr_rpc
lasr_types

GOAL: get them to separate git repo's

Halfway step:

- Reorg the `lasr` repo into a 

Actors all go into `lasr_node` crate
  - dependency to types
Clients
  - wallet client to cli
  - eo client to the node crate
