#![forbid(unsafe_code)]

use watchtower_rs::cgroup::get_running_container_id_from_string;

#[test]
fn get_running_container_id_returns_the_matching_container_id() {
    let cid = get_running_container_id_from_string(
        r#"
15:name=systemd:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
14:misc:/
13:rdma:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
12:pids:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
11:hugetlb:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
10:net_prio:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
9:perf_event:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
8:net_cls:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
7:freezer:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
6:devices:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
5:blkio:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
4:cpuacct:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
3:cpu:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
2:cpuset:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
1:memory:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
0::/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
        "#,
    )
    .expect("expected a container id to be extracted");

    assert_eq!(
        cid.as_str(),
        "991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377"
    );
}

#[test]
fn get_running_container_id_returns_none_when_no_matching_container_id_is_present() {
    assert_eq!(get_running_container_id_from_string("14:misc:/"), None);
}
