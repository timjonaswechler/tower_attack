use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraState};
use bevy::picking::prelude::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::{prelude::*, render::view::Hdr};
use core::f32::consts::PI;

#[derive(Component)]
pub struct Focusable;

// Bevy's FreeCamera pitch is negative when looking down from the default view.
const MIN_CAMERA_PITCH: f32 = -70.0 * PI / 180.0;
const MAX_CAMERA_PITCH: f32 = -10.0 * PI / 180.0;

// Plugin that spawns the camera.
pub struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera);
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.5, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Hdr,
        // Unfortunately, MSAA and HDR are not supported simultaneously under WebGL.
        // Since this example uses HDR, we must disable MSAA for Wasm builds, at least
        // until WebGPU is ready and no longer behind a feature flag in Web browsers.
        #[cfg(target_arch = "wasm32")]
        Msaa::Off,
        // This component stores all camera settings and state, which is used by the FreeCameraPlugin to
        // control it. These properties can be changed at runtime, but beware the controller system is
        // constantly using and modifying those values unless the enabled field is false.
        FreeCamera {
            sensitivity: 0.2,
            friction: 25.0,
            walk_speed: 3.0,
            run_speed: 100.0,
            ..default()
        },
    ));
}

// Plugin that handles camera settings controls and information text
pub struct CameraSettingsPlugin;

impl Plugin for CameraSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFocus>()
            .add_systems(PostStartup, spawn_text)
            .add_systems(
                Update,
                (
                    update_camera_settings,
                    constrain_camera_angle,
                    update_camera_focus,
                    update_text,
                )
                    .chain(),
            );
        #[cfg(debug_assertions)]
        app.add_systems(Update, debug_center_ray);
    }
}

#[derive(Component)]
struct InfoText;

fn spawn_text(mut commands: Commands, free_camera_query: Query<&FreeCamera>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(-16),
            left: px(12),
            ..default()
        },
        children![Text::new(format!(
            "{}",
            free_camera_query.single().unwrap()
        ))],
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(12),
            left: px(12),
            ..default()
        },
        children![Text::new(concat![
            "Z/X: decrease/increase sensitivity\n",
            "C/V: decrease/increase friction\n",
            "F/G: decrease/increase scroll factor\n",
            "B: enable/disable controller",
        ]),],
    ));

    // Mutable text marked with component
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            right: px(12),
            ..default()
        },
        children![(InfoText, Text::new(""))],
    ));
}

fn update_camera_settings(
    mut camera_query: Query<(&mut FreeCamera, &mut FreeCameraState)>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let (mut free_camera, mut free_camera_state) = camera_query.single_mut().unwrap();

    if input.pressed(KeyCode::KeyZ) {
        free_camera.sensitivity = (free_camera.sensitivity - 0.005).max(0.005);
    }
    if input.pressed(KeyCode::KeyX) {
        free_camera.sensitivity += 0.005;
    }
    if input.pressed(KeyCode::KeyC) {
        free_camera.friction = (free_camera.friction - 0.2).max(0.0);
    }
    if input.pressed(KeyCode::KeyV) {
        free_camera.friction += 0.2;
    }
    if input.pressed(KeyCode::KeyF) {
        free_camera.scroll_factor = (free_camera.scroll_factor - 0.02).max(0.02);
    }
    if input.pressed(KeyCode::KeyG) {
        free_camera.scroll_factor += 0.02;
    }
    if input.just_pressed(KeyCode::KeyB) {
        free_camera_state.enabled = !free_camera_state.enabled;
    }
}

fn constrain_camera_angle(
    mut camera_query: Query<(&mut Transform, &mut FreeCameraState), With<Camera3d>>,
) {
    let Ok((mut transform, mut free_camera_state)) = camera_query.single_mut() else {
        return;
    };

    let clamped_pitch = free_camera_state
        .pitch
        .clamp(MIN_CAMERA_PITCH, MAX_CAMERA_PITCH);
    if clamped_pitch != free_camera_state.pitch {
        free_camera_state.pitch = clamped_pitch;
        transform.rotation = Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            free_camera_state.yaw,
            free_camera_state.pitch,
        );
    }
}

fn update_text(
    mut text_query: Query<&mut Text, With<InfoText>>,
    camera_query: Query<(&FreeCamera, &FreeCameraState, &Transform)>,
    focus: Res<CameraFocus>,
) {
    let mut text = text_query.single_mut().unwrap();
    let (free_camera, free_camera_state, transform) = camera_query.single().unwrap();

    let look_at_text = if let Some(p) = focus.hit_point {
        format!("Look At Hit: ({:.02}, {:.02}, {:.02})", p.x, p.y, p.z)
    } else {
        "Look At Hit: none".to_string()
    };

    text.0 = format!(
        "Enabled: {},\nSensitivity: {:.03}\nFriction: {:.01}\nScroll factor: {:.02}\nWalk Speed: {:.02}\nRun Speed: {:.02}\nSpeed: {:.02}\nPosition: ({:.02}, {:.02}, {:.02})\nRotation: ({:.02}, {:.02}, {:.02})\n{}",
        free_camera_state.enabled,
        free_camera.sensitivity,
        free_camera.friction,
        free_camera.scroll_factor,
        free_camera.walk_speed,
        free_camera.run_speed,
        free_camera_state.velocity.length(),
        transform.translation.x,
        transform.translation.y,
        transform.translation.z,
        transform.rotation.x,
        transform.rotation.y,
        transform.rotation.z,
        look_at_text,
    );
}

fn debug_center_ray(focus: Res<CameraFocus>, mut gizmos: Gizmos) {
    if let Some(point) = focus.hit_point {
        gizmos.sphere(point, 0.12, Color::srgb(1.0, 1.0, 1.0));
    }
}

#[derive(Resource, Default, Debug)]
pub struct CameraFocus {
    pub hit_point: Option<Vec3>,
    pub hit_entity: Option<Entity>,
}

#[derive(Clone, Copy)]
struct LastValidCameraState {
    translation: Vec3,
    rotation: Quat,
    pitch: f32,
    yaw: f32,
}

fn update_camera_focus(
    window: Single<&Window>,
    camera_q: Single<(&Camera, &mut Transform, &mut FreeCameraState), With<Camera3d>>,
    focusable_q: Query<(), With<Focusable>>,
    mut ray_cast: MeshRayCast,
    mut focus: ResMut<CameraFocus>,
    mut last_valid_camera_state: Local<Option<LastValidCameraState>>,
) {
    let (camera, mut camera_transform, mut free_camera_state) = camera_q.into_inner();

    let screen_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let filter = |entity| focusable_q.contains(entity);
    let settings = MeshRayCastSettings::default()
        // Hidden focus targets should still be focusable. Non-`Focusable` meshes are ignored,
        // so objects between the camera and the ground do not block this ray.
        .with_visibility(RayCastVisibility::Any)
        .with_filter(&filter)
        .always_early_exit();

    let mut cast_focus_ray = |camera_transform: &Transform| {
        let camera_global_transform = GlobalTransform::from(*camera_transform);
        let Ok(ray) = camera.viewport_to_world(&camera_global_transform, screen_center) else {
            return None;
        };

        ray_cast
            .cast_ray(ray, &settings)
            .first()
            .map(|(entity, hit)| (*entity, hit.point))
    };

    let hit = if let Some(hit) = cast_focus_ray(&camera_transform) {
        *last_valid_camera_state = Some(LastValidCameraState {
            translation: camera_transform.translation,
            rotation: camera_transform.rotation,
            pitch: free_camera_state.pitch,
            yaw: free_camera_state.yaw,
        });
        Some(hit)
    } else if let Some(last_valid_state) = *last_valid_camera_state {
        // The camera moved or rotated so the center ray no longer hits focusable ground.
        // Restore the last valid pose and stop velocity so movement cannot keep pushing outward.
        camera_transform.translation = last_valid_state.translation;
        camera_transform.rotation = last_valid_state.rotation;
        free_camera_state.pitch = last_valid_state.pitch;
        free_camera_state.yaw = last_valid_state.yaw;
        free_camera_state.velocity = Vec3::ZERO;
        cast_focus_ray(&camera_transform)
    } else {
        None
    };

    if let Some((entity, point)) = hit {
        focus.hit_point = Some(point);
        focus.hit_entity = Some(entity);
    } else {
        focus.hit_point = None;
        focus.hit_entity = None;
    }
}
