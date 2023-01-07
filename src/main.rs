use std::{f32::consts::PI, ops::RangeInclusive};

use bevy::{prelude::*, sprite::Anchor, utils::FloatOrd};
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_rapier2d::prelude::*;
use iyes_loopless::{
    prelude::{AppLooplessStateExt, ConditionSet, IntoConditionalSystem},
    state::NextState,
};
use rand::prelude::random;

const SCREEN_WIDTH: f32 = 1280.0;
const SCREEN_HEIGHT: f32 = 720.0;

#[derive(Component)]
struct GameUi;

#[derive(Component)]
struct Bounds;

#[derive(Component)]
struct ScoreText;

#[derive(Component)]
struct ObstacleBundle;

#[derive(Component)]
struct Bird {
    alive: bool,
}

#[derive(Component)]
struct Wall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GameState {
    Running,
    Ended,
}

#[derive(Resource)]
struct PauseState(bool);

#[derive(Resource)]
struct Score(i32);

impl Default for Score {
    fn default() -> Self {
        Self(0)
    }
}

#[derive(Resource)]
struct ObstacleConfig {
    min_x_between: f32,
    y_mid_range: RangeInclusive<f32>,
    y_mid_offset: f32,
}

impl Default for ObstacleConfig {
    fn default() -> Self {
        Self {
            min_x_between: 650.0,
            y_mid_range: -100.0..=100.0,
            y_mid_offset: 125.0,
        }
    }
}

fn main() {
    App::new()
        // Plugins
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    window: WindowDescriptor {
                        width: SCREEN_WIDTH,
                        height: SCREEN_HEIGHT,
                        title: "Bevy Bird".to_string(),
                        resizable: false,
                        ..Default::default()
                    },
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugin(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .add_plugin(RapierDebugRenderPlugin::default())
        .add_plugin(WorldInspectorPlugin)
        // Constant Resources
        .insert_resource(RapierConfiguration {
            gravity: Vec2::Y * -9.81 * 45.0,
            ..default()
        })
        .insert_resource(ClearColor(Color::rgb_u8(173, 230, 255)))
        .insert_resource(PauseState(false))
        // Global setup
        .add_loopless_state(GameState::Running)
        .add_startup_system(setup_camera)
        // Game Running
        .add_enter_system(GameState::Running, add_resource::<ObstacleConfig>)
        .add_enter_system(GameState::Running, add_resource::<Score>)
        .add_enter_system(GameState::Running, setup_bird)
        .add_enter_system(GameState::Running, setup_bounds)
        .add_enter_system(GameState::Running, setup_ui)
        .add_enter_system(GameState::Running, reset_camera)
        .add_system(toggle_pause)
        .add_system_set(
            ConditionSet::new()
                .run_in_state(GameState::Running)
                .run_if_not(is_paused)
                .with_system(tiling_background)
                .with_system(jump_on_space)
                .with_system(camera_follows_bird)
                .with_system(spawn_obstacles)
                .with_system(despawn_offscreen_obstacles)
                .with_system(increment_score.run_on_event::<CollisionEvent>())
                .with_system(kill_bird_on_collision.run_on_event::<ContactForceEvent>())
                .with_system(bounds_follow_bird)
                .with_system(bird_rotates_with_velocity)
                .with_system(update_score_text)
                .with_system(score_increases_difficulty)
                .with_system(end_on_bird_leaves_screen)
                .into(),
        )
        .add_exit_system(GameState::Running, despawn_components::<Bird>)
        .add_exit_system(GameState::Running, despawn_components::<ObstacleBundle>)
        .add_exit_system(GameState::Running, despawn_components::<Bounds>)
        .add_exit_system(GameState::Running, despawn_components::<GameUi>)
        .add_exit_system(GameState::Running, despawn_components::<TilingBackground>)
        .add_exit_system(GameState::Running, remove_resource::<ObstacleConfig>)
        .add_exit_system(GameState::Running, remove_resource::<Score>)
        // Game Ended
        .add_enter_system(GameState::Ended, immediately_restart_game)
        .run();
}

fn is_paused(pause_state: Res<PauseState>) -> bool {
    pause_state.0
}

fn despawn_components<T: Component>(mut commands: Commands, q: Query<Entity, With<T>>) {
    for e in q.iter() {
        commands.entity(e).despawn_recursive();
    }
}

fn remove_resource<T: Resource>(mut commands: Commands) {
    commands.remove_resource::<T>();
}

fn add_resource<T: Resource + Default>(mut commands: Commands) {
    commands.insert_resource(T::default());
}

fn immediately_restart_game(mut commands: Commands) {
    commands.insert_resource(NextState(GameState::Running));
}

fn bird_rotates_with_velocity(mut bird_query: Query<(&Bird, &mut Transform, &Velocity)>) {
    let (bird, mut transform, velocity) = bird_query.single_mut();
    if !bird.alive {
        return;
    }

    let normalized_velocity = velocity.linvel.normalize();
    let mut rotation = normalized_velocity.y.atan2(normalized_velocity.x);
    if rotation.is_nan() {
        rotation = 0.0;
    }
    rotation = rotation * PI / 180.0 * 5.0;
    transform.rotation.z = rotation;
}

fn spawn_obstacles(
    mut commands: Commands,
    bird_query: Query<&Transform, (With<Bird>, Without<Camera2d>)>,
    obstacle_query: Query<(&Transform, &ObstacleBundle)>,
    obstacle_config: Res<ObstacleConfig>,
    assets: Res<AssetServer>,
) {
    let max_obstacle_x: f32 = obstacle_query
        .iter()
        .map(|(t, _)| FloatOrd(t.translation.x))
        .max()
        .unwrap_or(FloatOrd(f32::MIN))
        .0;

    let bird = bird_query.single();
    let bird_x = bird.translation.x;

    let screen_edge = bird_x + SCREEN_WIDTH / 2.0;
    let spawn_at = screen_edge + 50.0;

    if spawn_at - max_obstacle_x < obstacle_config.min_x_between {
        return;
    }

    let width = 82.0;
    let y_mid = random::<f32>()
        * (obstacle_config.y_mid_range.end() - obstacle_config.y_mid_range.start())
        + obstacle_config.y_mid_range.start();

    commands
        .spawn(ObstacleBundle)
        .insert(SpatialBundle {
            transform: Transform::from_xyz(spawn_at, y_mid, 0.0),
            ..default()
        })
        .insert(Name::new(format!("Obstacle @ {spawn_at}")))
        .add_children(|commands| {
            commands
                .spawn(SpriteBundle {
                    sprite: Sprite {
                        anchor: Anchor::Center,
                        custom_size: Some(Vec2::new(width, obstacle_config.y_mid_offset * 2.0)),
                        ..default()
                    },
                    texture: assets.load("images/ring_over.png"),
                    transform: Transform::from_xyz(0.0, 0.0, 2.0),
                    ..default()
                })
                .insert(Name::new("Ring Over"));

            commands
                .spawn(SpriteBundle {
                    sprite: Sprite {
                        anchor: Anchor::Center,
                        custom_size: Some(Vec2::new(width, obstacle_config.y_mid_offset * 2.0)),
                        ..default()
                    },
                    texture: assets.load("images/ring_under.png"),
                    transform: Transform::from_xyz(0.0, 0.0, 0.9),
                    ..default()
                })
                .insert(Name::new("Ring Under"));

            let collider_height = 15.0;

            commands
                .spawn(Collider::cuboid(width / 5.0, collider_height))
                .insert(TransformBundle::from(Transform::from_xyz(
                    0.0,
                    obstacle_config.y_mid_offset - collider_height,
                    0.0,
                )))
                .insert(Wall)
                .insert(Name::new(format!("Up @ {spawn_at}")));

            commands
                .spawn(Collider::cuboid(width / 5.0, collider_height))
                .insert(TransformBundle::from(Transform::from_xyz(
                    0.0,
                    -1.0 * obstacle_config.y_mid_offset + collider_height,
                    0.0,
                )))
                .insert(Wall)
                .insert(Name::new(format!("Down @ {spawn_at}")));

            commands
                .spawn(Collider::cuboid(10.0, obstacle_config.y_mid_offset))
                .insert(TransformBundle::from(Transform::from_xyz(0.0, 0.0, 0.0)))
                .insert(Sensor)
                .insert(ActiveEvents::COLLISION_EVENTS)
                .insert(Name::new(format!("Sensor @ {spawn_at}")));
        });
}

#[derive(Component)]
struct TilingBackground;

// TODO: make bidrectional as it currently only tiles to the right. sometimes bugs out when the bird bounces far back
fn tiling_background(
    mut commands: Commands,
    camera_query: Query<&mut Transform, (With<Camera2d>, Without<TilingBackground>)>,
    backgrounds_query: Query<(Entity, &mut Transform), (With<TilingBackground>, Without<Camera2d>)>,
    assets: Res<AssetServer>,
) {
    let camera = camera_query.single();

    // center-left anchors, with the width of one screen
    let max_covered_x = backgrounds_query
        .iter()
        .map(|(_, t)| FloatOrd(t.translation.x + SCREEN_WIDTH))
        .max()
        .unwrap_or(FloatOrd(0.0))
        .0;

    let cover_until_x = camera.translation.x + SCREEN_WIDTH;

    if max_covered_x < cover_until_x {
        commands
            .spawn(SpriteBundle {
                sprite: Sprite {
                    // anchor: Anchor::CenterLeft,
                    custom_size: Some(Vec2::new(SCREEN_WIDTH, 2048.0)),
                    ..default()
                },
                transform: Transform::from_xyz(max_covered_x, 0.0, 0.1),
                texture: assets.load("images/bg_layer3.png"),
                ..default()
            })
            .insert(Name::new(format!("Tiling Background @ {max_covered_x}")))
            .insert(TilingBackground);
    }

    for (e, t) in backgrounds_query.iter() {
        if t.translation.x < camera.translation.x - SCREEN_WIDTH * 2.0 {
            commands.entity(e).despawn_recursive();
        }
    }
}

fn despawn_offscreen_obstacles(
    mut commands: Commands,
    bird_query: Query<&Transform, (With<Bird>, Without<Camera2d>)>,
    obstacle_query: Query<(Entity, &Transform, &ObstacleBundle)>,
) {
    let bird = bird_query.single();
    let screen_edge = bird.translation.x - SCREEN_WIDTH / 2.0;
    let despawn_from = screen_edge - 50.0;

    for (entity, transform, _) in obstacle_query.iter() {
        if transform.translation.x >= despawn_from {
            continue;
        }
        commands.entity(entity).despawn_recursive();
    }
}

fn bounds_follow_bird(
    mut bounds_query: Query<&mut Transform, (With<Bounds>, Without<Bird>)>,
    bird_query: Query<&Transform, (With<Bird>, Without<Bounds>)>,
) {
    let bird = bird_query.single();
    let bird_x = bird.translation.x;

    for mut bounds in bounds_query.iter_mut() {
        bounds.translation.x = bird_x - SCREEN_WIDTH / 2.0;
    }
}

fn setup_bounds(mut commands: Commands) {
    commands
        .spawn(Collider::cuboid(SCREEN_WIDTH, 10.0))
        .insert(Bounds)
        .insert(Name::new("Upper Bound"))
        .insert(TransformBundle::from(Transform::from_xyz(
            0.0,
            -1.0 * SCREEN_HEIGHT / 2.0,
            0.0,
        )));

    commands
        .spawn(Collider::cuboid(SCREEN_WIDTH, 10.0))
        .insert(Bounds)
        .insert(Name::new("Lower Bound"))
        .insert(TransformBundle::from(Transform::from_xyz(
            0.0,
            SCREEN_HEIGHT / 2.0,
            0.0,
        )));
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn reset_camera(mut camera_query: Query<&mut Transform, With<Camera2d>>) {
    let mut camera = camera_query.single_mut();
    camera.translation.x = 0.0;
}

fn camera_follows_bird(
    mut camera_query: Query<&mut Transform, (With<Camera2d>, Without<Bird>)>,
    bird_query: Query<&Transform, (With<Bird>, Without<Camera2d>)>,
) {
    let mut camera = camera_query.single_mut();
    let bird = bird_query.single();
    camera.translation.x = bird.translation.x;
}

fn setup_bird(mut commands: Commands, assets: Res<AssetServer>) {
    commands
        .spawn(RigidBody::Dynamic)
        .insert(SpriteBundle {
            sprite: Sprite {
                anchor: Anchor::Center,
                custom_size: Some(Vec2::new(100.0, 50.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 1.0),
            texture: assets.load("images/player.png"),
            ..default()
        })
        .insert(Collider::ball(25.0))
        .insert(Sleeping::disabled())
        .insert(Name::new("Bird"))
        .insert(Velocity::zero())
        .insert(ExternalImpulse::default())
        .insert(Restitution::coefficient(2.0))
        .insert(ActiveEvents::CONTACT_FORCE_EVENTS)
        .insert(ExternalForce {
            force: Vec2::X * 5.0,
            ..default()
        })
        .insert(CollisionGroups::new(Group::ALL, Group::ALL))
        .insert(Bird { alive: true });
}

fn toggle_pause(
    keys: Res<Input<KeyCode>>,
    mut pause_state: ResMut<PauseState>,
    mut rapier_configuration: ResMut<RapierConfiguration>,
) {
    if keys.just_pressed(KeyCode::P) {
        pause_state.0 = !pause_state.0;
        rapier_configuration.timestep_mode = TimestepMode::Variable {
            max_dt: 1.0 / 60.0,
            time_scale: if pause_state.0 { 0.0 } else { 1.0 },
            substeps: 1,
        };
    }
}

fn jump_on_space(
    keys: Res<Input<KeyCode>>,
    mut bird_query: Query<(&mut Velocity, &mut ExternalImpulse, &Bird)>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    let (mut v, mut i, b) = bird_query.single_mut();
    if !b.alive {
        return;
    }

    v.linvel.y = 0.0;
    i.impulse = Vec2::Y * 50.0;
}

fn increment_score(mut collision_events: EventReader<CollisionEvent>, mut score: ResMut<Score>) {
    for collision_event in collision_events.iter() {
        let CollisionEvent::Started(_, _, _) = collision_event else {
            continue
        };

        score.0 += 1;
    }
}

fn kill_bird_on_collision(mut bird_query: Query<(&mut CollisionGroups, &mut Bird)>) {
    let (mut bird_collision_groups, mut bird) = bird_query.single_mut();

    // Bird no longer collides with anything
    bird_collision_groups.memberships = Group::NONE;
    bird_collision_groups.filters = Group::NONE;
    bird.alive = false;
}

fn end_on_bird_leaves_screen(bird_query: Query<&Transform, With<Bird>>, mut commands: Commands) {
    let bird = bird_query.single();
    let bird_y = bird.translation.y;
    let margin = 50.0;
    if bird_y < -1.0 * SCREEN_HEIGHT / 2.0 - margin || bird_y > SCREEN_HEIGHT / 2.0 + margin {
        println!("Bird dead");
        commands.insert_resource(NextState(GameState::Ended));
    }
}

fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands
        .spawn(NodeBundle {
            style: Style {
                size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                ..default()
            },
            background_color: Color::NONE.into(),
            ..default()
        })
        .insert(GameUi)
        .insert(Name::new("Game UI"))
        .add_children(|commands| {
            commands
                .spawn(
                    TextBundle::from_section(
                        "Score: 0",
                        TextStyle {
                            font: asset_server.load("fonts/OpenSans-Regular.ttf"),
                            font_size: 30.0,
                            color: Color::BLACK,
                        },
                    )
                    .with_text_alignment(TextAlignment::TOP_LEFT)
                    .with_style(Style {
                        position_type: PositionType::Absolute,
                        position: UiRect {
                            top: Val::Px(10.0),
                            left: Val::Px(10.0),
                            ..default()
                        },
                        ..default()
                    }),
                )
                .insert(Name::new("Score Text"))
                .insert(ScoreText);
        });
}

fn update_score_text(mut score_text_query: Query<&mut Text, With<ScoreText>>, score: Res<Score>) {
    if !score.is_changed() {
        return;
    }

    let score = score.0;
    let mut score_text = score_text_query.single_mut();
    score_text.sections[0].value = format!("Score: {score}");
}

fn score_increases_difficulty(mut obstacle_config: ResMut<ObstacleConfig>, score: Res<Score>) {
    if !score.is_changed() {
        return;
    }

    let score = score.0 as f32;
    let default_obstacle_config = ObstacleConfig::default();

    obstacle_config.min_x_between = 500.0_f32.max(default_obstacle_config.min_x_between - score);
    obstacle_config.y_mid_offset = 100.0_f32.max(default_obstacle_config.y_mid_offset - score);
    obstacle_config.y_mid_range = (-200.0_f32
        .max(default_obstacle_config.y_mid_range.start() - score))
        ..=(200.0_f32.min(default_obstacle_config.y_mid_range.end() + score));
}
