{ ... }:
{
  networking.hostName = "lasr-debug-server";

  # Add your SSH key and comment your username/system-name
  users.users.root.openssh.authorizedKeys.keys = [
    # eureka-cpu
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINQ66bGgeELzU/wZjpYxSlKIgMoROQxPx76vGdpS3lwc github.eureka@gmail.com" # dev-one
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAsUzc7Wg9FwImAMPc61K/zO9gvUDJVHwQ0+GTrO1mqJ github.eureka@gmail.com" # critter-tank
  ];
}
