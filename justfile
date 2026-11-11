default:
    @just --list

setup:
    mise x -- cargo xtask setup

build:
    mise x -- cargo xtask build

check:
    mise x -- cargo xtask check

apple *args:
    mise x -- cargo xtask apple {{ args }}

format:
    mise x -- cargo xtask format

release-check:
    mise x -- cargo xtask release-check

proofs *args:
    mise x -- cargo xtask proofs {{ args }}

artifacts:
    mise x -- cargo xtask artifacts

editing-assurance *args:
    mise x -- cargo xtask editing-assurance {{ args }}

demo-notes *args:
    mise x -- cargo xtask demo-notes {{ args }}

scale-profile *args:
    mise x -- cargo xtask scale-profile {{ args }}

browser-performance *args:
    mise x -- cargo xtask browser-performance {{ args }}

memory-profile *args:
    mise x -- cargo xtask memory-profile {{ args }}

storage-profile *args:
    mise x -- cargo xtask storage-profile {{ args }}

storage-summary *args:
    mise x -- cargo xtask storage-summary {{ args }}
