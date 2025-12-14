#version 450

// Per-instance attributes (binding 0, per instance)
layout(location = 0) in ivec2 pos;
layout(location = 1) in ivec2 size;
layout(location = 2) in float d;

layout(location = 0) out vec2 texCoord;

layout(binding = 0) uniform UniformData {
    ivec2 aspect;
} uniformData;

// 4 vertices per instance: top-left, top-right, bottom-left, bottom-right
const ivec2 vertices[4] = ivec2[4](
    ivec2(0, 0),  // top-left
    ivec2(1, 0),  // top-right
    ivec2(0, 1),  // bottom-left
    ivec2(1, 1)   // bottom-right
);

void main() {
    ivec2 vertexOffset = vertices[gl_VertexIndex];
    ivec2 vertexPos = pos + ivec2(vertexOffset * size);

    vec2 normalized = vec2(vertexPos) / vec2(uniformData.aspect);
    vec2 ndc = normalized * 2.0 - 1.0;

    gl_Position = vec4(ndc, d, 1.0);
    texCoord = vertexOffset;
}