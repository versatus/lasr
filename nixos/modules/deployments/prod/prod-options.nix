{ releaseCycle, ... }:
{
  # TODO: Finalize production options for lasr_node server
  networking.hostName = "lasr-${releaseCycle}-server";
}
