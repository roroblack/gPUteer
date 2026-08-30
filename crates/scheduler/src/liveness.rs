//! 노드 생존 판정 — **관측을 정리할 뿐 사망을 선언하지 않는다.**
//!
//! # 규범이 먼저 정해 둔 것
//!
//! `ADR-033` §7 이 이 층을 두 개로 나눴다.
//!
//! ```text
//! 관측(신고)   같은 풀의 이웃이 "저 노드에 연락이 안 된다" 고 보고한다
//! 판정(결정)   Broker 가 그 보고를 모아 노드 상태를 정하고 재배정을 결정한다
//! ```
//!
//! ★ 그리고 못박았다 — **"연락이 안 된다" 는 "죽었다" 가 아니다.**
//!   네트워크가 갈라졌으면 대상 노드는 멀쩡히 계속 실행 중이다.
//!   그 상태에서 다른 GPU 에 같은 작업을 다시 띄우면 **두 번 돈다.**
//!   `side_effecting` 작업이면 되돌릴 수 없다.
//!
//! 그래서 이 커널은 `Dead` 라는 값을 **아예 갖고 있지 않다.** 가장
//! 나쁜 판정이 `Silent`(정해진 시간 안에 소식이 없다)이고, 그것은
//! 사실 진술이지 결정이 아니다. 재배정 결정은 `ADR-033` §8 의 여섯
//! 조건이 갖춰진 뒤에야 가능하고, 그건 이 조각 밖이다.
//!
//! # 순수 커널이다
//!
//! 시계·I/O·난수·전역 상태를 쓰지 않는다. `now_unix_ms` 를 인자로
//! 받고, 정렬에 `BTreeMap` 을 써서 입력 순서와 무관하게 같은 답을 낸다
//! — 이 저장소의 다른 kernel(`evaluate_effective_replicas`,
//! `gpu_scope_candidate`)과 같은 규칙이다.
//!
//! # 이 커널이 하지 않는 것
//!
//! ```text
//! 서명 검증        호출부가 Verified<NodeHeartbeat> 로 이미 했다.
//!                  여기서 흉내 내면 강제하지 못하는 것을 강제한다고
//!                  주장하게 된다(§0.4)
//! 재배정 결정      ADR-033 §8 의 여섯 조건이 필요하다. 이 커널은
//!                  "다시 띄워도 된다" 를 절대 말하지 않는다
//! 이웃 신고 취합   §7 의 신고 경로는 아직 메시지가 없다. 없는 입력을
//!                  받는 척하지 않는다
//! 사망 선언        Dead 라는 값 자체가 없다 — 위 참조
//! ```

use std::collections::{BTreeMap, BTreeSet};

/// 서명 검증을 마친 heartbeat 하나에서 뽑은 사실.
///
/// ★ `Verified<pb::NodeHeartbeat>` 를 직접 받지 않는다. 그러면 이
///   크레이트가 protocol·crypto 에 묶여 순수 커널이 아니게 된다.
///   호출부가 검증 후 이 형태로 옮겨 담는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeartbeatObservation {
    /// 어느 노드인가.
    pub node_id: String,
    /// 어느 장치가 서명했는가. 검증된 서명자와 같아야 한다 —
    /// **그 대조는 호출부 책임이다.**
    pub device_id: String,
    /// 그 노드가 스스로 찍은 시각.
    pub issued_at_unix_ms: u64,
    /// 그 노드가 들고 있다고 보고한 Lease 세대. 0 이면 없다.
    pub fence_epoch: u64,
    /// 그 노드에서 돈다고 보고한 attempt 수.
    pub running_attempts: u32,
}

/// 판정 기준. **정책값이며 측정값이 아니다.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivenessPolicy {
    /// 이 시간 안에 소식이 있으면 `Live`.
    pub live_within_ms: u64,
    /// 이 시간을 넘으면 `Silent`.
    ///
    /// ★ `live_within_ms` 와 같은 값을 쓰지 않는다. 둘 사이 구간이
    ///   `Suspect` 다 — 경계 하나로 뒤집히면 시계가 몇 밀리초만 흔들려도
    ///   판정이 오간다.
    pub silent_after_ms: u64,
}

/// 판정 결과. **`Dead` 가 없다** — 모듈 문서 참조.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NodeLiveness {
    /// 최근에 소식이 있었다.
    Live,
    /// 늦었지만 아직 침묵으로 볼 시간은 아니다.
    Suspect,
    /// 정해진 시간 안에 소식이 없다. **죽었다는 뜻이 아니다.**
    Silent,
    /// 이 노드의 관측이 아예 없다. `Silent` 와 다르다 — 한 번도 못 본
    /// 노드와, 보다가 끊긴 노드는 원인이 다르다.
    NoObservation,
}

/// 판정을 거부하는 이유.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LivenessError {
    /// 정책이 말이 안 된다.
    PolicyInverted {
        live_within_ms: u64,
        silent_after_ms: u64,
    },
    /// 식별자가 비었다.
    ///
    /// ★ 빈 문자열을 통과시키면 서로 다른 노드가 하나로 뭉친다.
    ///   `evaluate_eligibility` 가 같은 결함으로 검수에 걸린 적이 있다.
    BlankIdentity { field: &'static str },
    /// 같은 노드를 서로 다른 장치가 보고했다.
    ///
    /// ★ 조용히 하나를 고르면 안 된다 — 어느 쪽이 진짜인지 이 커널은
    ///   모른다. 판정을 멈추고 호출부에 알린다.
    ConflictingDevice {
        node_id: String,
        first: String,
        second: String,
    },
}

impl std::fmt::Display for LivenessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PolicyInverted {
                live_within_ms,
                silent_after_ms,
            } => write!(
                f,
                "LIVENESS_POLICY_INVERTED: silent_after_ms({silent_after_ms}) 가 \
                 live_within_ms({live_within_ms}) 보다 크지 않다"
            ),
            Self::BlankIdentity { field } => {
                write!(f, "LIVENESS_BLANK_IDENTITY: {field} 가 비었다")
            }
            Self::ConflictingDevice {
                node_id,
                first,
                second,
            } => write!(
                f,
                "LIVENESS_CONFLICTING_DEVICE: node {node_id} 를 {first} 와 {second} 가 \
                 각각 보고했다 — 어느 쪽이 진짜인지 이 커널은 모른다"
            ),
        }
    }
}

impl std::error::Error for LivenessError {}

/// 노드 하나의 판정 결과와 근거.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeLivenessReport {
    pub node_id: String,
    pub liveness: NodeLiveness,
    /// 판정에 쓴 관측. 없으면 `None`.
    ///
    /// ★ 근거를 같이 낸다. 결과만 주면 왜 그렇게 판정했는지 나중에
    ///   재구성할 수 없다 — `CLAUDE.md` §1 의 provenance 규칙과 같은 뜻이다.
    pub selected: Option<HeartbeatObservation>,
    /// 마지막 소식으로부터 흐른 시간. 관측이 없으면 `None`.
    pub silence_ms: Option<u64>,
}

/// 관측들을 노드별로 정리해 판정한다.
///
/// # 어느 관측을 쓰는가
///
/// 같은 노드에 관측이 여럿이면 **`issued_at_unix_ms` 가 가장 큰 것**을
/// 쓴다. 동점이면 `fence_epoch` 이 큰 것 — 같은 순간에 두 세대가 보고됐다면
/// 나중 세대가 지금 상태다.
///
/// ★ 그래도 완전 동점이면 `running_attempts` 로 가른다. 임의로 하나를
///   고르면 입력 순서에 따라 답이 달라져 kernel 이 결정적이지 않게 된다.
///
/// # 미래의 관측
///
/// ★ `issued_at_unix_ms > now` 는 **버리지 않고 `Live` 로 본다.**
///   보내는 쪽 시계가 조금 앞선 것뿐인데 버리면, 시계가 빠른 노드가
///   영영 `Silent` 로 보인다. 침묵 시간은 0 으로 센다 — 음수를 만들지
///   않는다.
pub fn classify_node_liveness(
    observations: &[HeartbeatObservation],
    known_nodes: &[String],
    policy: LivenessPolicy,
    now_unix_ms: u64,
) -> Result<Vec<NodeLivenessReport>, LivenessError> {
    if policy.silent_after_ms <= policy.live_within_ms {
        return Err(LivenessError::PolicyInverted {
            live_within_ms: policy.live_within_ms,
            silent_after_ms: policy.silent_after_ms,
        });
    }

    let mut selected: BTreeMap<String, HeartbeatObservation> = BTreeMap::new();
    for observation in observations {
        if observation.node_id.trim().is_empty() {
            return Err(LivenessError::BlankIdentity { field: "node_id" });
        }
        if observation.device_id.trim().is_empty() {
            return Err(LivenessError::BlankIdentity { field: "device_id" });
        }
        match selected.get(&observation.node_id) {
            None => {
                selected.insert(observation.node_id.clone(), observation.clone());
            }
            Some(existing) => {
                if existing.device_id != observation.device_id {
                    return Err(LivenessError::ConflictingDevice {
                        node_id: observation.node_id.clone(),
                        first: existing.device_id.clone(),
                        second: observation.device_id.clone(),
                    });
                }
                if rank(observation) > rank(existing) {
                    selected.insert(observation.node_id.clone(), observation.clone());
                }
            }
        }
    }

    // 관측이 있는 노드 + 알려진 노드 전부를 대상으로 한다. 알려졌는데
    // 관측이 없는 노드를 빼면 "안 보이는 노드" 가 결과에서 사라진다 —
    // 그게 가장 알아야 할 노드다.
    let mut all: BTreeSet<String> = selected.keys().cloned().collect();
    for node_id in known_nodes {
        if node_id.trim().is_empty() {
            return Err(LivenessError::BlankIdentity {
                field: "known node_id",
            });
        }
        all.insert(node_id.clone());
    }

    Ok(all
        .into_iter()
        .map(|node_id| match selected.get(&node_id) {
            None => NodeLivenessReport {
                node_id,
                liveness: NodeLiveness::NoObservation,
                selected: None,
                silence_ms: None,
            },
            Some(observation) => {
                let silence = now_unix_ms.saturating_sub(observation.issued_at_unix_ms);
                let liveness = if silence <= policy.live_within_ms {
                    NodeLiveness::Live
                } else if silence < policy.silent_after_ms {
                    NodeLiveness::Suspect
                } else {
                    NodeLiveness::Silent
                };
                NodeLivenessReport {
                    node_id,
                    liveness,
                    selected: Some(observation.clone()),
                    silence_ms: Some(silence),
                }
            }
        })
        .collect())
}

/// 같은 노드의 관측 중 무엇이 더 최신인가.
fn rank(observation: &HeartbeatObservation) -> (u64, u64, u32) {
    (
        observation.issued_at_unix_ms,
        observation.fence_epoch,
        observation.running_attempts,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> LivenessPolicy {
        LivenessPolicy {
            live_within_ms: 30_000,
            silent_after_ms: 90_000,
        }
    }

    fn observation(node: &str, issued_at: u64) -> HeartbeatObservation {
        HeartbeatObservation {
            node_id: node.to_string(),
            device_id: format!("{node}-device"),
            issued_at_unix_ms: issued_at,
            fence_epoch: 7,
            running_attempts: 1,
        }
    }

    /// 세 구간이 실제로 갈리는가.
    #[test]
    fn the_three_bands_are_distinguished() {
        let now = 1_000_000;
        let cases = [
            (now - 1_000, NodeLiveness::Live),
            (now - 30_000, NodeLiveness::Live),
            (now - 30_001, NodeLiveness::Suspect),
            (now - 89_999, NodeLiveness::Suspect),
            (now - 90_000, NodeLiveness::Silent),
            (now - 500_000, NodeLiveness::Silent),
        ];
        for (issued_at, expected) in cases {
            let reports =
                classify_node_liveness(&[observation("n1", issued_at)], &[], policy(), now)
                    .expect("판정");
            assert_eq!(
                reports[0].liveness, expected,
                "issued_at={issued_at} 에서 판정이 다르다"
            );
        }
    }

    /// ★ 사망 판정이 존재하지 않는가.
    ///
    /// `ADR-033` §7 이 "연락이 안 된다 != 죽었다" 를 못박았다. 가장
    /// 나쁜 값이 `Silent` 여야 하고, 그 위는 없어야 한다.
    #[test]
    fn the_worst_verdict_is_silence_not_death() {
        let reports =
            classify_node_liveness(&[observation("n1", 0)], &[], policy(), u64::MAX / 2)
                .expect("판정");
        assert_eq!(
            reports[0].liveness,
            NodeLiveness::Silent,
            "아무리 오래 침묵해도 Silent 를 넘는 판정은 없어야 한다"
        );
    }

    /// 관측이 없는 알려진 노드가 결과에서 사라지지 않는가.
    ///
    /// ★ 빠지면 "안 보이는 노드" 를 아무도 못 본다 — 그게 가장 알아야
    ///   할 노드다.
    #[test]
    fn a_known_node_with_no_observation_still_appears() {
        let reports = classify_node_liveness(
            &[observation("n1", 999_000)],
            &["n1".to_string(), "n2".to_string()],
            policy(),
            1_000_000,
        )
        .expect("판정");
        assert_eq!(reports.len(), 2);
        let n2 = reports.iter().find(|r| r.node_id == "n2").expect("n2");
        assert_eq!(n2.liveness, NodeLiveness::NoObservation);
        assert_eq!(n2.silence_ms, None, "관측이 없으면 침묵 시간도 없다");
    }

    /// 입력 순서가 결과를 바꾸지 않는가.
    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let now = 1_000_000;
        let mut set = vec![
            observation("n1", now - 10_000),
            observation("n1", now - 50_000),
            observation("n2", now - 5_000),
        ];
        let first = classify_node_liveness(&set, &[], policy(), now).expect("판정");
        set.reverse();
        let second = classify_node_liveness(&set, &[], policy(), now).expect("판정");
        assert_eq!(first, second, "입력 순서에 따라 답이 달라진다");
        // 가장 최신 관측이 선택됐는가.
        let n1 = first.iter().find(|r| r.node_id == "n1").expect("n1");
        assert_eq!(n1.silence_ms, Some(10_000));
    }

    /// 시계가 앞선 노드가 영영 침묵으로 보이지 않는가.
    #[test]
    fn a_clock_slightly_ahead_does_not_look_silent() {
        let now = 1_000_000;
        let reports = classify_node_liveness(&[observation("n1", now + 5_000)], &[], policy(), now)
            .expect("판정");
        assert_eq!(reports[0].liveness, NodeLiveness::Live);
        assert_eq!(
            reports[0].silence_ms,
            Some(0),
            "미래 관측의 침묵 시간은 음수가 아니라 0 이어야 한다"
        );
    }

    /// 뒤집힌 정책은 거부하는가.
    #[test]
    fn an_inverted_policy_is_refused() {
        for (live, silent) in [(90_000u64, 30_000u64), (30_000, 30_000)] {
            let error = classify_node_liveness(
                &[],
                &[],
                LivenessPolicy {
                    live_within_ms: live,
                    silent_after_ms: silent,
                },
                0,
            )
            .expect_err("뒤집힌 정책이 통과했다");
            assert!(matches!(error, LivenessError::PolicyInverted { .. }));
        }
    }

    /// 빈 식별자를 fail-closed 하는가.
    #[test]
    fn blank_identities_are_refused() {
        let mut blank_node = observation("n1", 0);
        blank_node.node_id = "  ".to_string();
        assert!(matches!(
            classify_node_liveness(&[blank_node], &[], policy(), 0),
            Err(LivenessError::BlankIdentity { field: "node_id" })
        ));

        let mut blank_device = observation("n1", 0);
        blank_device.device_id = String::new();
        assert!(matches!(
            classify_node_liveness(&[blank_device], &[], policy(), 0),
            Err(LivenessError::BlankIdentity { field: "device_id" })
        ));

        assert!(matches!(
            classify_node_liveness(&[], &[String::new()], policy(), 0),
            Err(LivenessError::BlankIdentity {
                field: "known node_id"
            })
        ));
    }

    /// 같은 노드를 다른 장치가 보고하면 멈추는가.
    #[test]
    fn conflicting_devices_stop_the_judgment() {
        let mut other = observation("n1", 10);
        other.device_id = "someone-else".to_string();
        let error = classify_node_liveness(&[observation("n1", 0), other], &[], policy(), 0)
            .expect_err("충돌이 통과했다");
        assert!(matches!(error, LivenessError::ConflictingDevice { .. }));
    }

    /// 같은 시각의 두 관측을 결정적으로 가르는가.
    #[test]
    fn a_tie_on_time_is_broken_deterministically() {
        let now = 1_000;
        let mut older_epoch = observation("n1", 500);
        older_epoch.fence_epoch = 3;
        let mut newer_epoch = observation("n1", 500);
        newer_epoch.fence_epoch = 9;

        let a = classify_node_liveness(
            &[older_epoch.clone(), newer_epoch.clone()],
            &[],
            policy(),
            now,
        )
        .expect("판정");
        let b =
            classify_node_liveness(&[newer_epoch, older_epoch], &[], policy(), now).expect("판정");
        assert_eq!(a, b);
        assert_eq!(
            a[0].selected.as_ref().expect("selected").fence_epoch,
            9,
            "같은 시각이면 나중 세대가 지금 상태다"
        );
    }
}
