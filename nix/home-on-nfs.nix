{
  pkgs,
  rewind,
}:
pkgs.testers.nixosTest {
  name = "home-on-nfs";

  nodes = {
    server = { ... }: {
      services.nfs.server.enable = true;
      services.nfs.server.exports = {
        "/home" = {
          "*" = [
            "rw"
            "no_root_squash"
            "fsid=0"
          ];
        };
      };
      networking.firewall.allowedTCPPorts = [ 2049 ];
    };

    client = { ... }: {
      users.users.alice.isNormalUser = true;
      environment.systemPackages = [ rewind ];
      virtualisation.fileSystems = {
        "/home" = {
          device = "server:/";
          fsType = "nfs";
          options = [ "vers=4" ];
        };
      };
    };
  };

  testScript = ''
    server.wait_for_unit("nfs-server")
    start_all()

    client.wait_for_unit("home.mount")
    client.succeed("mkdir -p /home/alice && chown alice /home/alice")

    with subtest("rewind should work with $HOME on NFS"):
        out = client.fail("su alice -c 'rewind export does-not-exist' 2>&1")
        assert 'no run matches "does-not-exist";' in out, f"rewind failed for another reason:\n{out}"
  '';
}
