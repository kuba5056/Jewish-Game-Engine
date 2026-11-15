// Rolls a ball with WASD or the arrow keys, jumps with Space, and keeps the
// camera on it. Used by assets/scenes/scripted.ron.

fn init() {
    this.speed = 6.0;
    this.jump = 6.0;
    this.start = this.entity.position;
}

// Movement belongs in fixed_update, in step with the physics.
fn fixed_update(dt) {
    let me = this.entity;

    let wish = vec3(0, 0, 0);
    if key_down("D") || key_down("Right") { wish.x += 1.0; }
    if key_down("A") || key_down("Left")  { wish.x -= 1.0; }
    if key_down("S") || key_down("Down")  { wish.z += 1.0; }
    if key_down("W") || key_down("Up")    { wish.z -= 1.0; }
    wish = wish.normalized() * this.speed;

    // Steer on the ground plane; leave falling to gravity.
    let velocity = me.velocity;
    velocity.x = wish.x;
    velocity.z = wish.z;

    // The ray starts just under the ball, so it only finds the ground.
    if key_down("Space") {
        let ground = raycast(me.position + vec3(0, -0.45, 0), vec3(0, -1, 0), 0.15);
        if ground != () { velocity.y = this.jump; }
    }
    me.velocity = velocity;

    // Fell off the world: back to the start.
    if me.position.y < -6.0 {
        me.position = this.start;
        me.velocity = vec3(0, 0, 0);
        play_sound("sounds/fall.wav", 0.6);
    }
}

fn update(dt) {
    let focus = this.entity.position;
    set_camera(focus + vec3(0, 8, 9), focus);
}
