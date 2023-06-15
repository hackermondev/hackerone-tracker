{ pkgs }: {
        deps = [
                pkgs.redis
                pkgs.rustc 
                pkgs.cargo
                pkgs.toybox
                pkgs.openssl
                pkgs.jq.bin
                pkgs.yq-go
        ];
}