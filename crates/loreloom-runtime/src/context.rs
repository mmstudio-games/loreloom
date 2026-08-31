use loreloom_core::{
    CharacterContext, GoalStatus, SceneContext, SceneObservation, TranscriptItemRecord,
};

use crate::ContextProjectionPolicy;

pub(crate) fn project_observation(
    observation: &mut SceneObservation,
    policy: ContextProjectionPolicy,
) {
    let character = project_character(&mut observation.player, policy);
    let scene = project_scene(&mut observation.scene, policy);
    let transcript = project_transcript(&mut observation.recent_transcript, policy);
    observation.truncated |= character || scene || transcript;
}

pub(crate) fn project_npc_context(
    character: &mut CharacterContext,
    scene: &mut SceneContext,
    recent_dialogue: &mut Vec<TranscriptItemRecord>,
    policy: ContextProjectionPolicy,
) -> bool {
    project_character(character, policy)
        | project_scene(scene, policy)
        | project_transcript(recent_dialogue, policy)
}

fn project_character(character: &mut CharacterContext, policy: ContextProjectionPolicy) -> bool {
    character.known_facts.sort_by(|left, right| {
        right
            .last_confirmed_at
            .cmp(&left.last_confirmed_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    character.goals.sort_by(|left, right| {
        goal_status_order(left.status)
            .cmp(&goal_status_order(right.status))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    truncate(&mut character.known_facts, policy.known_facts)
        | truncate(&mut character.goals, policy.goals)
        | truncate(&mut character.inventory, policy.inventory_items)
        | truncate(&mut character.skills, policy.skills)
}

fn project_scene(scene: &mut SceneContext, policy: ContextProjectionPolicy) -> bool {
    truncate(&mut scene.visible_actors, policy.visible_actors)
}

fn project_transcript(
    transcript: &mut Vec<TranscriptItemRecord>,
    policy: ContextProjectionPolicy,
) -> bool {
    let mut truncated = false;
    while transcript.len() > policy.transcript_items {
        transcript.remove(0);
        truncated = true;
    }
    let mut bytes = transcript
        .iter()
        .map(|item| item.text.as_str().len())
        .sum::<usize>();
    while bytes > policy.transcript_bytes {
        let Some(first) = transcript.first() else {
            break;
        };
        bytes = bytes.saturating_sub(first.text.as_str().len());
        transcript.remove(0);
        truncated = true;
    }
    truncated
}

fn truncate<T>(values: &mut Vec<T>, limit: usize) -> bool {
    if values.len() <= limit {
        return false;
    }
    values.truncate(limit);
    true
}

const fn goal_status_order(status: GoalStatus) -> u8 {
    match status {
        GoalStatus::Active => 0,
        GoalStatus::Blocked => 1,
        GoalStatus::Achieved => 2,
        GoalStatus::Abandoned => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::project_transcript;
    use crate::ContextProjectionPolicy;
    use loreloom_core::{
        LongText, Revision, SessionId, TranscriptItemId, TranscriptItemRecord, TranscriptSpeaker,
        TranscriptState,
    };

    fn parse<T: std::str::FromStr>(value: &str) -> T
    where
        T::Err: std::fmt::Debug,
    {
        value.parse().expect("fixture identifier")
    }

    fn transcript(id: &str, text: &str) -> TranscriptItemRecord {
        TranscriptItemRecord {
            id: parse::<TranscriptItemId>(id),
            session_id: parse::<SessionId>("ses_01890f6a-2b30-7d4e-8f90-123456789abc"),
            revision: Some(Revision::new(1)),
            speaker: TranscriptSpeaker::Narrator,
            text: LongText::new(text).expect("transcript text"),
            state: TranscriptState::Committed,
            supporting_events: Vec::new(),
        }
    }

    #[test]
    fn transcript_projection_keeps_the_newest_count_and_byte_window() {
        let mut values = vec![
            transcript("trn_01890f6a-2b31-7d4e-8f90-123456789abc", "old"),
            transcript("trn_01890f6a-2b32-7d4e-8f90-123456789abc", "middle"),
            transcript("trn_01890f6a-2b33-7d4e-8f90-123456789abc", "new"),
        ];
        let truncated = project_transcript(
            &mut values,
            ContextProjectionPolicy {
                transcript_items: 2,
                transcript_bytes: 3,
                ..ContextProjectionPolicy::default()
            },
        );
        assert!(truncated);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].text.as_str(), "new");
    }
}
