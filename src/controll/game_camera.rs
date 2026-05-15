use super::component::Focusable;
use bevy::{
    app::{App, Plugin, RunFixedMainLoop, RunFixedMainLoopSystems},
    camera::Camera,
    camera::Camera3d,
    color::Color,
    ecs::prelude::*,
    gizmos::gizmos::Gizmos,
    input::ButtonInput,
    input::keyboard::KeyCode,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseScrollUnit},
    log::info,
    math::Curve,
    math::curve::{Interval, SampleAutoCurve},
    math::{Dir3, EulerRot, Quat, StableInterpolate, Vec2, Vec3},
    picking::pointer::PointerInteraction,
    picking::prelude::{MeshRayCast, MeshRayCastSettings, RayCastVisibility},
    time::{Real, Time},
    transform::prelude::{GlobalTransform, Transform},
    window::{CursorGrabMode, CursorOptions, Window},
};

use core::{f32::consts::*, fmt};

/// A freecam-style camera controller plugin.
///
/// Use the [`FreeCamera`] struct to add and customize the controller for a camera entity.
/// The camera's dynamic state is managed by the [`FreeCameraState`] struct.
pub struct FreeCameraPlugin;

impl Plugin for FreeCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFocus>();

        // This ordering is required so that both fixed update and update systems can see the results correctly
        app.add_systems(
            RunFixedMainLoop,
            (
                run_freecamera_controller,
                rotate_freecam_to,
                keep_camera_inside_focusable_area,
            )
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        );

        #[cfg(debug_assertions)]
        app.add_systems(
            RunFixedMainLoop,
            debug_center_ray.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        );
    }
}

/// Scales mouse motion into yaw/pitch movement.
///
/// Based on Valorant's default sensitivity, not entirely sure why it is exactly 1.0 / 180.0,
/// but we're guessing it is a misunderstanding between degrees/radians and then sticking with
/// it because it felt nice.
const RADIANS_PER_DOT: f32 = 1.0 / 180.0;

// Bevy's FreeCamera pitch is negative when looking down from the default view.
const MIN_CAMERA_PITCH: f32 = -70.0 * PI / 180.0;
const MAX_CAMERA_PITCH: f32 = -25.0 * PI / 180.0;

/// Stores the settings for the [`FreeCamera`] controller.
///
/// This component defines static configuration for camera controls,
/// including movement speed, sensitivity, and input bindings.
///
/// From the controller’s perspective, this data is treated as immutable,
/// but it may be modified externally (e.g., by a settings UI) at runtime.
///
/// Add this component to a [`Camera`] entity to enable `FreeCamera` controls.
/// The associated dynamic state is automatically handled by [`FreeCameraState`],
/// which is added to the entity as a required component.
///
/// To activate the controller, add the [`FreeCameraPlugin`] to your [`App`].
#[derive(Component, Clone)]
#[require(FreeCameraState)]
pub struct FreeCamera {
    /// Multiplier for pitch and yaw rotation speed.
    pub sensitivity: f32,
    /// [`KeyCode`] for forward translation.
    pub key_forward: KeyCode,
    /// [`KeyCode`] for backward translation.
    pub key_back: KeyCode,
    /// [`KeyCode`] for left translation.
    pub key_left: KeyCode,
    /// [`KeyCode`] for right translation.
    pub key_right: KeyCode,
    /// [`KeyCode`] for up translation.
    pub key_up: KeyCode,
    /// [`KeyCode`] for down translation.
    pub key_down: KeyCode,
    /// [`KeyCode`] to use [`run_speed`](FreeCamera::run_speed) instead of
    /// [`walk_speed`](FreeCamera::walk_speed) for translation.
    pub key_run: KeyCode,
    /// [`MouseButton`] for grabbing the mouse focus.
    pub mouse_key_cursor_grab: MouseButton,
    /// [`MouseButton`] for grabbing the mouse focus only when the pointer is not over a pickable entity.
    pub mouse_key_empty_space_rotate: MouseButton,
    /// [`KeyCode`] for grabbing the keyboard focus.
    pub keyboard_key_toggle_cursor_grab: KeyCode,
    /// Modifier [`KeyCode`] for making pressed axis alignment buttons go in opposite direction
    pub key_snap_reverse: KeyCode,
    /// [`KeyCode`] for snapping camera to top/bottom (+Y/-Y).
    pub axis_top: KeyCode,
    /// [`KeyCode`] for snapping camera to right/left (+X/-X).
    pub axis_right: KeyCode,
    /// [`KeyCode`] for snapping camera to front/back (-Z/+Z).
    pub axis_front: KeyCode,
    /// Base multiplier for unmodified translation speed.
    pub walk_speed: f32,
    /// Base multiplier for running translation speed.
    pub run_speed: f32,
    /// Multiplier for how much the mouse scroll wheel affects [`walk_speed`](FreeCamera::walk_speed)
    /// and [`run_speed`](FreeCamera::run_speed).
    ///
    /// Mouse scroll affects speed exponentially. This is to ensure that scrolling the same
    /// amount always has the same effect on speed, regardless of how the scroll amount
    /// is reported by the hardware (i.e. as one big event vs many smaller events). This
    /// also allows the free camera to navigate very large scenes easier.
    ///
    /// For every unit of scroll, the speed of the camera is multiplied by a factor of
    /// `e^(scroll_factor)`.
    ///
    /// A reasonable value to start with is a `scroll_factor` between 0.04879016 (~ln(1.05))
    /// and 0.0953102 (~ln(1.1)). They represent an increase by a factor between 1.05 and 1.1 per
    /// positive unit scroll and a reduction between ~0.952 (~e^-0.04879016) and ~0.909
    /// (~e^-0.0953102) times its value per negative unit scroll
    ///
    /// A `scroll_factor` closer to 0.0 means that speed will be less sensitive to scroll.
    /// A `scroll_factor` equal to 0.0 means that speed is unaffected by scroll
    /// (it will be multiplied by a factor of 1.0 per positive and negative unit scroll).
    pub scroll_factor: f32,
    /// Friction factor used to exponentially decay [`velocity`](FreeCameraState::velocity) over time.
    pub friction: f32,
    /// Speed of camera rotation to snapped axis in radians/second
    pub rotation_speed: f32,
}

impl Default for FreeCamera {
    fn default() -> Self {
        Self {
            sensitivity: 0.2,
            key_forward: KeyCode::KeyW,
            key_back: KeyCode::KeyS,
            key_left: KeyCode::KeyA,
            key_right: KeyCode::KeyD,
            key_up: KeyCode::KeyE,
            key_down: KeyCode::KeyQ,
            key_run: KeyCode::ShiftLeft,
            mouse_key_cursor_grab: MouseButton::Right,
            mouse_key_empty_space_rotate: MouseButton::Left,
            keyboard_key_toggle_cursor_grab: KeyCode::KeyM,
            key_snap_reverse: KeyCode::ControlLeft,
            axis_top: KeyCode::Numpad7,
            axis_right: KeyCode::Numpad3,
            axis_front: KeyCode::Numpad1,
            walk_speed: 5.0,
            run_speed: 15.0,
            // Approximation of ln(1.05)
            scroll_factor: 0.04879016,
            friction: 40.0,
            rotation_speed: PI / 16.0 * 60.0,
        }
    }
}

impl fmt::Display for FreeCamera {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "
Freecamera Controls:
    Mouse\t- Move camera orientation
    Scroll\t- Adjust movement speed
    {:?}\t- Hold to grab cursor
    {:?}\t- Hold to rotate on empty space
    {:?}\t- Toggle cursor grab
    {:?} & {:?}\t- Fly forward & backwards
    {:?} & {:?}\t- Fly sideways left & right
    {:?} & {:?}\t- Fly up & down
    {:?}\t- Fly faster while held
    [{:?} + ]{:?}\t- Snap to Up (+Y)/Down (-Y)
    [{:?} + ]{:?}\t- Snap to Right (+X)/Left (-X)
    [{:?} + ]{:?}\t- Snap to Front (-Z)/Back (+Z)",
            self.mouse_key_cursor_grab,
            self.mouse_key_empty_space_rotate,
            self.keyboard_key_toggle_cursor_grab,
            self.key_forward,
            self.key_back,
            self.key_left,
            self.key_right,
            self.key_up,
            self.key_down,
            self.key_run,
            self.key_snap_reverse,
            self.axis_top,
            self.key_snap_reverse,
            self.axis_right,
            self.key_snap_reverse,
            self.axis_front,
        )
    }
}

/// Tracks the runtime state of a [`FreeCamera`] controller.
///
/// This component holds dynamic data that changes during camera operation,
/// such as pitch, yaw, velocity, and whether the controller is currently enabled.
///
/// It is automatically added to any entity that has a [`FreeCamera`] component,
/// and is updated by the [`FreeCameraPlugin`] systems in response to user input.
#[derive(Component)]
pub struct FreeCameraState {
    /// Enables [`FreeCamera`] controls when `true`.
    pub enabled: bool,
    /// Internal flag indicating if this controller has been initialized by the [`FreeCameraPlugin`].
    initialized: bool,
    /// This [`FreeCamera`]'s pitch rotation.
    pub pitch: f32,
    /// This [`FreeCamera`]'s yaw rotation.
    pub yaw: f32,
    /// This [`FreeCamera`]'s translation velocity.
    pub velocity: Vec3,
    /// Dictates camera movement during camera snap at speed, specified in [`FreeCamera`] by [`FreeCamera::rotation_speed`] field.
    /// Consist of counter of seconds from pressing curve snap hotkeys and curve that used to interpolate between old and new rotation
    pub rotation_curve: Option<(f32, SampleAutoCurve<Quat>)>,
}

impl Default for FreeCameraState {
    fn default() -> Self {
        Self {
            enabled: true,
            initialized: false,
            pitch: 0.0,
            yaw: 0.0,
            velocity: Vec3::ZERO,
            rotation_curve: None,
        }
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

/// Updates the camera's position and orientation based on user input.
///
/// - [`FreeCamera`] contains static configuration such as key bindings, movement speed, and sensitivity.
/// - [`FreeCameraState`] stores the dynamic runtime state, including pitch, yaw, velocity, and enable flags.
///
/// This system is typically added via the [`FreeCameraPlugin`].
///
/// Axis snapping takes priority over mouse movement.
pub fn run_freecamera_controller(
    time: Res<Time<Real>>,
    mut windows: Query<(&Window, &mut CursorOptions)>,
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    accumulated_mouse_scroll: Res<AccumulatedMouseScroll>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    pointer_interactions: Query<&PointerInteraction>,
    mut toggle_cursor_grab: Local<bool>,
    mut mouse_cursor_grab: Local<bool>,
    mut empty_space_mouse_rotate: Local<bool>,
    mut query: Query<(&mut Transform, &mut FreeCameraState, &FreeCamera), With<Camera>>,
) {
    let dt = time.delta_secs();

    let Ok((mut transform, mut state, config)) = query.single_mut() else {
        return;
    };

    if !state.initialized {
        let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::YXZ);
        state.yaw = yaw;
        state.pitch = pitch;
        state.initialized = true;
        info!("{}", *config);
    }

    if !state.enabled {
        // don't keep the cursor grabbed if the camera controller was disabled.
        if *toggle_cursor_grab || *mouse_cursor_grab || *empty_space_mouse_rotate {
            *toggle_cursor_grab = false;
            *mouse_cursor_grab = false;
            *empty_space_mouse_rotate = false;

            for (_, mut cursor_options) in &mut windows {
                cursor_options.grab_mode = CursorGrabMode::None;
                cursor_options.visible = true;
            }
        }
        return;
    }

    let _scroll = match accumulated_mouse_scroll.unit {
        MouseScrollUnit::Line => accumulated_mouse_scroll.delta.y,
        MouseScrollUnit::Pixel => {
            accumulated_mouse_scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR
        }
    };

    // Handle key input
    let mut axis_input = Vec3::ZERO;
    if key_input.pressed(config.key_forward) {
        axis_input.z += 1.0;
    }
    if key_input.pressed(config.key_back) {
        axis_input.z -= 1.0;
    }
    if key_input.pressed(config.key_right) {
        axis_input.x += 1.0;
    }
    if key_input.pressed(config.key_left) {
        axis_input.x -= 1.0;
    }
    if key_input.pressed(config.key_up) {
        axis_input.y += 1.0;
    }
    if key_input.pressed(config.key_down) {
        axis_input.y -= 1.0;
    }

    let mut cursor_grab_change = false;
    if key_input.just_pressed(config.keyboard_key_toggle_cursor_grab) {
        *toggle_cursor_grab = !*toggle_cursor_grab;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_pressed(config.mouse_key_cursor_grab) {
        *mouse_cursor_grab = true;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_released(config.mouse_key_cursor_grab) {
        *mouse_cursor_grab = false;
        cursor_grab_change = true;
    }

    let pointer_is_over_pickable = pointer_interactions
        .iter()
        .any(|interaction| interaction.get_nearest_hit().is_some());
    if mouse_button_input.just_pressed(config.mouse_key_empty_space_rotate)
        && !pointer_is_over_pickable
    {
        *empty_space_mouse_rotate = true;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_released(config.mouse_key_empty_space_rotate) {
        *empty_space_mouse_rotate = false;
        cursor_grab_change = true;
    }
    let cursor_grab = *mouse_cursor_grab || *empty_space_mouse_rotate || *toggle_cursor_grab;

    // Update velocity
    if axis_input != Vec3::ZERO {
        let max_speed = if key_input.pressed(config.key_run) {
            config.run_speed
        } else {
            config.walk_speed
        };

        state.velocity = axis_input.normalize() * max_speed;
    } else {
        let friction = config.friction.clamp(0.0, f32::MAX);
        state.velocity.smooth_nudge(&Vec3::ZERO, friction, dt);
        if state.velocity.length_squared() < 1e-6 {
            state.velocity = Vec3::ZERO;
        }
    }

    // Apply movement update
    if state.velocity != Vec3::ZERO {
        let yaw_rotation = Quat::from_rotation_y(state.yaw);
        let forward = yaw_rotation * Vec3::NEG_Z;
        let right = yaw_rotation * Vec3::X;
        let up = Vec3::Y;
        transform.translation += state.velocity.x * dt * right
            + state.velocity.y * dt * up
            + state.velocity.z * dt * forward;
    }

    // Handle cursor grab
    if cursor_grab_change {
        if cursor_grab {
            for (window, mut cursor_options) in &mut windows {
                if !window.focused {
                    continue;
                }

                cursor_options.grab_mode = CursorGrabMode::Locked;
                cursor_options.visible = false;
            }
        } else {
            for (_, mut cursor_options) in &mut windows {
                cursor_options.grab_mode = CursorGrabMode::None;
                cursor_options.visible = true;
            }
        }
    }

    // Handle mouse input
    if accumulated_mouse_motion.delta != Vec2::ZERO && cursor_grab {
        // Apply look update
        state.pitch = (state.pitch
            - accumulated_mouse_motion.delta.y * RADIANS_PER_DOT * config.sensitivity)
            .clamp(-PI / 2., PI / 2.);
        state.yaw -= accumulated_mouse_motion.delta.x * RADIANS_PER_DOT * config.sensitivity;
        transform.rotation = Quat::from_euler(EulerRot::ZYX, 0.0, state.yaw, state.pitch);
    }

    // Axis snapping
    let mod_key_pressed = key_input.pressed(config.key_snap_reverse);
    let mut rotate_to = None;
    if key_input.just_pressed(config.axis_front) {
        if mod_key_pressed {
            rotate_to = Some((Dir3::Z, Dir3::Y));
        } else {
            rotate_to = Some((Dir3::NEG_Z, Dir3::Y));
        }
    }
    if key_input.just_pressed(config.axis_right) {
        if mod_key_pressed {
            rotate_to = Some((Dir3::NEG_X, Dir3::Y));
        } else {
            rotate_to = Some((Dir3::X, Dir3::Y));
        }
    }
    if key_input.just_pressed(config.axis_top) {
        if mod_key_pressed {
            rotate_to = Some((Dir3::NEG_Y, Dir3::NEG_Z));
        } else {
            rotate_to = Some((Dir3::Y, Dir3::Z));
        }
    }
    if let Some((dir, up)) = rotate_to {
        let start = transform.rotation;
        let target = Transform::default().looking_to(dir, up).rotation; // I don't understand why Quat::look_to_rh produce different result.
        let angle = target.angle_between(start);
        let rotation_time = angle / config.rotation_speed;

        if let Ok(interval) = Interval::new(0.0, rotation_time) {
            let curve = SampleAutoCurve::new(interval, [start, target])
                .expect("Interval should be in bounds as start and end are finite numbers");
            state.rotation_curve = Some((0.0, curve));
        }
    }
}

/// Smoothly changes orientation([`Transform`]) of [`FreeCamera`] camera according to target orientation in [`FreeCameraState`].
///
/// - [`FreeCamera`] contains static configuration such as key bindings and rotation speed.
/// - [`FreeCameraState`] stores the dynamic runtime state, including direction for camera rotation and enable flags.
///
/// This system is typically added via the [`FreeCameraPlugin`].
pub fn rotate_freecam_to(
    mut query: Query<(&mut Transform, &mut FreeCameraState), With<Camera>>,
    time: Res<Time<Real>>,
) {
    let Ok((mut transform, mut state)) = query.single_mut() else {
        return;
    };
    if !state.enabled {
        return;
    }
    let Some((progress, curve)) = state.rotation_curve.as_mut() else {
        return;
    };
    *progress += time.delta_secs();
    transform.rotation = curve.sample_clamped(*progress);
    if !curve.domain().contains(*progress) {
        state.rotation_curve = None;
    }
    let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::YXZ);
    state.pitch = pitch;
    state.yaw = yaw;
}

fn keep_camera_inside_focusable_area(
    window: Single<&Window>,
    camera_q: Single<(&Camera, &mut Transform, &mut FreeCameraState), With<Camera3d>>,
    focusable_q: Query<(), With<Focusable>>,
    mut ray_cast: MeshRayCast,
    mut focus: ResMut<CameraFocus>,
    mut last_valid_camera_state: Local<Option<LastValidCameraState>>,
) {
    let (camera, mut camera_transform, mut free_camera_state) = camera_q.into_inner();

    let clamped_pitch = free_camera_state
        .pitch
        .clamp(MIN_CAMERA_PITCH, MAX_CAMERA_PITCH);
    if clamped_pitch != free_camera_state.pitch {
        free_camera_state.pitch = clamped_pitch;
        camera_transform.rotation = Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            free_camera_state.yaw,
            free_camera_state.pitch,
        );
    }

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

fn debug_center_ray(focus: Res<CameraFocus>, mut gizmos: Gizmos) {
    if let Some(point) = focus.hit_point {
        gizmos.sphere(point, 0.12, Color::WHITE);
    }
}
