//! Demonstrates a camera rig that can focus on hovered entities and apply a
//! rainbow outline shader effect to them.

use bevy::{
    input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel},
    math::{Vec3Swizzles, Vec4},
    pbr::{
        ExtendedMaterial, MaterialExtension, MaterialPlugin, MeshMaterial3d, OpaqueRendererMethod,
    },
    picking::prelude::*,
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

const SHADER_ASSET_PATH: &str = "shaders/rainbow_outline.wgsl";

/// Custom StandardMaterial + shader extension used to draw the rainbow glow.
type GlowMaterial = ExtendedMaterial<StandardMaterial, RainbowOutlineExtension>;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            MeshPickingPlugin,
            MaterialPlugin::<GlowMaterial>::default(),
        ))
        .init_resource::<HoveredEntity>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                focus_on_hovered_entity,
                apply_scroll_zoom,
                update_camera_controls,
                drive_outline_materials,
            ),
        )
        .run();
}

#[derive(Resource, Default)]
struct HoveredEntity(Option<Entity>);

#[derive(Component, Default)]
struct FocusTarget;

#[derive(Component)]
struct FocusCamera {
    focus: Option<Entity>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    min_distance: f32,
    max_distance: f32,
    orbit_sensitivity: Vec2,
    zoom_sensitivity: f32,
    pitch_limit: f32,
    free_move_speed: f32,
    free_fast_multiplier: f32,
}

impl Default for FocusCamera {
    fn default() -> Self {
        Self {
            focus: None,
            yaw: 0.0,
            pitch: -0.2,
            distance: 10.0,
            min_distance: 1.0,
            max_distance: 30.0,
            orbit_sensitivity: Vec2::new(0.01, 0.008),
            zoom_sensitivity: 1.0,
            pitch_limit: std::f32::consts::FRAC_PI_2 - 0.05,
            free_move_speed: 6.0,
            free_fast_multiplier: 3.0,
        }
    }
}

impl FocusCamera {
    fn focus_on(&mut self, target: Entity, camera_transform: &Transform, target_position: Vec3) {
        self.focus = Some(target);
        let offset = camera_transform.translation - target_position;
        let horizontal = offset.xz().length().max(f32::EPSILON);
        self.distance = offset.length().clamp(self.min_distance, self.max_distance);
        self.yaw = offset.x.atan2(offset.z);
        self.pitch = offset
            .y
            .atan2(horizontal)
            .clamp(-self.pitch_limit, self.pitch_limit);
    }

    fn offset_vector(&self) -> Vec3 {
        let horizontal = self.distance * self.pitch.cos();
        let x = horizontal * self.yaw.sin();
        let z = horizontal * self.yaw.cos();
        let y = self.distance * self.pitch.sin();
        Vec3::new(x, y, z)
    }

    fn sync_from_transform(&mut self, transform: &Transform) {
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        self.yaw = yaw;
        self.pitch = pitch.clamp(-self.pitch_limit, self.pitch_limit);
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
struct RainbowOutlineExtension {
    #[uniform(100)]
    glow_control: Vec4,
}

impl Default for RainbowOutlineExtension {
    fn default() -> Self {
        Self {
            glow_control: Vec4::new(0.0, 0.0, 2.5, 0.35),
        }
    }
}

impl RainbowOutlineExtension {
    fn set_glow_strength(&mut self, strength: f32) {
        self.glow_control.x = strength;
    }

    fn set_phase(&mut self, phase: f32) {
        self.glow_control.y = phase;
    }

    fn set_outline_width(&mut self, width: f32) {
        self.glow_control.w = width;
    }
}

impl MaterialExtension for RainbowOutlineExtension {
    fn fragment_shader() -> ShaderRef {
        SHADER_ASSET_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER_ASSET_PATH.into()
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 800.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Camera3d::default(),
        FocusCamera::default(),
        Transform::from_xyz(-6.0, 4.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Text::new(
            "Hover an object and left-click or press F to focus.\n\
             Right mouse drags orbit a focused target; use the scroll wheel to dolly.\n\
             Without a focus, hold right mouse to look and fly with WASD (Shift boosts). Press Esc to clear focus.",
        ),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(30.0, 30.0))),
        MeshMaterial3d(standard_materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.05, 0.08),
            perceptual_roughness: 0.95,
            cull_mode: None,
            ..default()
        })),
    ));

    spawn_focus_target(
        &mut commands,
        &mut meshes,
        &mut glow_materials,
        Mesh::from(Cuboid::new(1.8, 1.2, 1.0)),
        Color::srgb(0.95, 0.35, 0.45),
        Transform::from_xyz(-3.5, 0.6, -1.5),
        "Glow Cube",
    );

    spawn_focus_target(
        &mut commands,
        &mut meshes,
        &mut glow_materials,
        Mesh::from(Sphere::new(0.9)),
        Color::srgb(0.2, 0.75, 0.9),
        Transform::from_xyz(0.0, 1.1, 0.0),
        "Glow Sphere",
    );

    spawn_focus_target(
        &mut commands,
        &mut meshes,
        &mut glow_materials,
        Mesh::from(Capsule3d::new(0.35, 0.75)),
        Color::srgb(0.9, 0.8, 0.2),
        Transform::from_xyz(3.25, 0.9, 1.75).with_rotation(Quat::from_rotation_z(0.4)),
        "Glow Capsule",
    );
}

fn spawn_focus_target(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    glow_materials: &mut Assets<GlowMaterial>,
    mesh: Mesh,
    color: Color,
    transform: Transform,
    name: &'static str,
) {
    commands
        .spawn((
            Name::new(name),
            FocusTarget,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(glow_materials.add(glow_material(color))),
            transform,
        ))
        .observe(store_hover_on_enter)
        .observe(clear_hover_on_exit);
}

fn store_hover_on_enter(over: On<Pointer<Over>>, mut hovered: ResMut<HoveredEntity>) {
    hovered.0 = Some(over.entity);
}

fn clear_hover_on_exit(out: On<Pointer<Out>>, mut hovered: ResMut<HoveredEntity>) {
    if hovered.0 == Some(out.entity) {
        hovered.0 = None;
    }
}

fn focus_on_hovered_entity(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredEntity>,
    mut camera_query: Query<(&mut FocusCamera, &Transform)>,
    targets: Query<&GlobalTransform, With<FocusTarget>>,
) {
    let focus_requested =
        keys.just_pressed(KeyCode::KeyF) || mouse_buttons.just_pressed(MouseButton::Left);

    if !focus_requested {
        return;
    }

    let Some(target_entity) = hovered.0 else {
        return;
    };

    let Ok(target_transform) = targets.get(target_entity) else {
        return;
    };

    if let Ok((mut rig, camera_transform)) = camera_query.single_mut() {
        rig.focus_on(
            target_entity,
            camera_transform,
            target_transform.translation(),
        );
    }
}

fn apply_scroll_zoom(
    mut camera_query: Query<&mut FocusCamera>,
    mut mouse_wheel_reader: MessageReader<MouseWheel>,
) {
    let Ok(mut rig) = camera_query.single_mut() else {
        return;
    };

    if rig.focus.is_none() {
        return;
    }

    let mut scroll_delta = 0.0;
    for wheel in mouse_wheel_reader.read() {
        let unit = match wheel.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.05,
        };
        scroll_delta += wheel.y * unit;
    }

    if scroll_delta.abs() > f32::EPSILON {
        rig.distance = (rig.distance - scroll_delta * rig.zoom_sensitivity)
            .clamp(rig.min_distance, rig.max_distance);
    }
}

fn update_camera_controls(
    mut camera_query: Query<(&mut Transform, &mut FocusCamera)>,
    targets: Query<&GlobalTransform, With<FocusTarget>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut rig)) = camera_query.single_mut() else {
        return;
    };

    if keys.just_pressed(KeyCode::Escape) {
        rig.sync_from_transform(&transform);
        rig.focus = None;
    }

    if let Some(target_entity) = rig.focus {
        if let Ok(target_transform) = targets.get(target_entity) {
            if mouse_buttons.pressed(MouseButton::Right) {
                let delta = mouse_motion.delta;
                rig.yaw -= delta.x * rig.orbit_sensitivity.x;
                rig.pitch = (rig.pitch + delta.y * rig.orbit_sensitivity.y)
                    .clamp(-rig.pitch_limit, rig.pitch_limit);
            }

            let target_position = target_transform.translation();
            transform.translation = target_position + rig.offset_vector();
            transform.look_at(target_position, Vec3::Y);
            return;
        } else {
            rig.sync_from_transform(&transform);
            rig.focus = None;
        }
    }

    if mouse_buttons.pressed(MouseButton::Right) {
        let delta = mouse_motion.delta;
        rig.yaw -= delta.x * rig.orbit_sensitivity.x;
        rig.pitch = (rig.pitch - delta.y * rig.orbit_sensitivity.y)
            .clamp(-rig.pitch_limit, rig.pitch_limit);
    }

    transform.rotation = Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, 0.0);

    let forward = transform.forward();
    let right = transform.right();
    let forward_flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let right_flat = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();

    let mut direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction += forward_flat;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= forward_flat;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += right_flat;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= right_flat;
    }

    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        let mut speed = rig.free_move_speed;
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            speed *= rig.free_fast_multiplier;
        }
        transform.translation += direction * speed * time.delta_secs();
    }
}

fn drive_outline_materials(
    time: Res<Time>,
    camera: Query<&FocusCamera>,
    mut materials: ResMut<Assets<GlowMaterial>>,
    targets: Query<(Entity, &MeshMaterial3d<GlowMaterial>), With<FocusTarget>>,
) {
    let Ok(rig) = camera.single() else {
        return;
    };

    let phase = time.elapsed_secs();
    for (entity, material_handle) in &targets {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.extension.set_phase(phase);
            let glow_strength = if Some(entity) == rig.focus { 1.0 } else { 0.0 };
            material.extension.set_glow_strength(glow_strength);
            material.extension.set_outline_width(0.45);
        }
    }
}

fn glow_material(color: Color) -> GlowMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            base_color: color.into(),
            perceptual_roughness: 0.5,
            opaque_render_method: OpaqueRendererMethod::Auto,
            ..default()
        },
        extension: RainbowOutlineExtension::default(),
    }
}
