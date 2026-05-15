mod controll;

use bevy::{
    camera_controller::free_camera::FreeCameraPlugin,
    color::palettes::tailwind::*,
    picking::{pointer::PointerInteraction, prelude::*},
    prelude::*,
    render::view::Hdr,
};

fn main() {
    let mut app = App::new();

    app.add_plugins((
        DefaultPlugins,
        FreeCameraPlugin,
        MeshPickingPlugin,
        controll::game_camera::FreeCameraPlugin,
    ))
    .add_systems(Startup, (setup, spawn_camera))
    .add_systems(PostUpdate, draw_mesh_intersections);

    app.run();
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
        controll::game_camera::FreeCamera {
            sensitivity: 0.2,
            friction: 25.0,
            ..default()
        },
    ));
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    _asset_server: Res<AssetServer>,
) {
    // Chessboard Plane
    let black_material = materials.add(Color::BLACK);
    let white_material = materials.add(Color::WHITE);
    let hover_matl = materials.add(Color::from(CYAN_300));
    let pressed_matl = materials.add(Color::from(YELLOW_300));
    let border_material = materials.add(Color::from(RED_600));
    let border_material2 = materials.add(Color::from(RED_900));

    let tile_size = 2.0;
    let board_size = 8;

    let plane_mesh = meshes.add(Plane3d::default().mesh().size(tile_size, tile_size));

    for i in 0..board_size * board_size {
        let x = i % board_size;
        let z = i / board_size;
        let is_border = x == 0 || x == board_size - 1 || z == 0 || z == board_size - 1;
        let odd = (x + z) % 2 == 0;
        let material = if is_border && !odd {
            border_material.clone()
        } else if is_border && odd {
            border_material2.clone()
        } else if odd {
            black_material.clone()
        } else {
            white_material.clone()
        };

        let mut plane = commands.spawn((
            Mesh3d(plane_mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x as f32 * tile_size, -1.0, z as f32 * tile_size),
            controll::component::Focusable,
        ));

        if is_border {
            plane.insert(controll::component::Border);
        } else {
            plane
                .observe(update_material_on::<Pointer<Over>>(hover_matl.clone()))
                .observe(update_material_on::<Pointer<Press>>(pressed_matl.clone()))
                .observe(update_material_on::<Pointer<Release>>(hover_matl.clone()))
                .observe(update_material_on::<Pointer<Out>>(material));
        }
    }

    // Light
    commands.spawn((PointLight::default(), Transform::from_xyz(4.0, 8.0, 4.0)));
}

fn update_material_on<E: EntityEvent>(
    new_material: Handle<StandardMaterial>,
) -> impl Fn(On<E>, Query<&mut MeshMaterial3d<StandardMaterial>>) {
    // An observer closure that captures `new_material`. We do this to avoid needing to write four
    // versions of this observer, each triggered by a different event and with a different hardcoded
    // material. Instead, the event type is a generic, and the material is passed in.
    move |event, mut query| {
        if let Ok(mut material) = query.get_mut(event.event_target()) {
            material.0 = new_material.clone();
        }
    }
}

/// A system that draws hit indicators for every pointer.
fn draw_mesh_intersections(pointers: Query<&PointerInteraction>, mut gizmos: Gizmos) {
    for (point, normal) in pointers
        .iter()
        .filter_map(|interaction| interaction.get_nearest_hit())
        .filter_map(|(_entity, hit)| hit.position.zip(hit.normal))
    {
        gizmos.sphere(point, 0.05, RED_500);
        gizmos.arrow(point, point + normal.normalize() * 0.5, PINK_100);
    }
}
