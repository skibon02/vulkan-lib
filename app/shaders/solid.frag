#version 450 core

layout(location = 0) in vec4 fragColor;

layout(location = 0) out vec4 outColor;

//layout(binding = 1) uniform Color {
//    vec3 color;
//} color;
//
//layout(binding = 2) uniform sampler2D tex;

void main() {
//    outColor = vec4(fragColor + color.color + texture(tex, gl_FragCoord.xy / 512.0).xyz, 1.0);
    outColor = fragColor;
}