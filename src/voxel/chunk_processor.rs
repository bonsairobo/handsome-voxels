use crate::{
    assets::IndexedPosColorNormVertices,
    collision::voxel_bvt::{generate_chunk_bvt, LatticeBVT, VoxelBVT},
    voxel::{
        double_buffer::DirtyChunks,
        meshing::{
            generate_mesh_vertices_with_greedy_quads, generate_mesh_vertices_with_surface_nets,
            loader::VoxelMeshLoader, manager::VoxelMeshManager,
        },
        VoxelAssets, VoxelMap,
    },
};

use amethyst::{assets::ProgressCounter, core::ecs::prelude::*};
use ilattice3 as lat;
use ilattice3::prelude::*;
use rayon::prelude::*;

#[cfg(feature = "profiler")]
use thread_profiler::profile_scope;

pub enum MeshMode {
    SurfaceNets,
    GreedyQuads,
}

pub struct VoxelChunkProcessorSystem;

impl<'a> System<'a> for VoxelChunkProcessorSystem {
    #[allow(clippy::type_complexity)]
    type SystemData = (
        ReadExpect<'a, VoxelMap>,
        ReadExpect<'a, MeshMode>,
        Write<'a, Option<DirtyChunks>>,
        WriteExpect<'a, VoxelAssets>,
        WriteExpect<'a, VoxelBVT>,
        VoxelMeshLoader<'a>,
        VoxelMeshManager<'a>,
    );

    fn run(
        &mut self,
        (
            map, mesh_mode, mut dirty_chunks, mut voxel_assets, mut voxel_bvt, loader, mut manager
        ): Self::SystemData,
    ) {
        #[cfg(feature = "profiler")]
        profile_scope!("voxel_chunk_processor");

        let chunks_to_generate = match dirty_chunks.take() {
            Some(c) => c.chunks,
            None => return,
        };

        let VoxelAssets {
            array_materials,
            meshes,
            ..
        } = &mut *voxel_assets;

        // Do parallel isosurface generation.
        let generated_chunks: Vec<(
            lat::Point,
            Option<LatticeBVT>,
            Option<IndexedPosColorNormVertices>,
        )> = chunks_to_generate
            .into_par_iter()
            .map(|chunk_key| {
                // TODO: figure out how to avoid copying like this; it's pretty slow
                let chunk_voxels = map.voxels.get_chunk_and_boundary(&chunk_key);

                let vertices = match *mesh_mode {
                    MeshMode::SurfaceNets => {
                        generate_mesh_vertices_with_surface_nets(&chunk_voxels)
                    }
                    MeshMode::GreedyQuads => {
                        generate_mesh_vertices_with_greedy_quads(&chunk_voxels)
                    }
                };

                let new_bvt = generate_chunk_bvt(&chunk_voxels, chunk_voxels.get_extent());

                (chunk_key, new_bvt, vertices)
            })
            .collect();

        // Collect the generated results.
        for (chunk_key, bvt, vertices) in generated_chunks.into_iter() {
            // Load the mesh.
            let mesh = {
                #[cfg(feature = "profiler")]
                profile_scope!("load_chunk_mesh");

                let mut _unused_progress = ProgressCounter::new();
                vertices.map(|v| loader.start_loading_chunk(v, &mut _unused_progress))
            };

            // Replace the chunk BVT.
            if let Some(bvt) = bvt {
                voxel_bvt.insert_chunk(chunk_key, bvt);
            } else {
                voxel_bvt.remove_chunk(&chunk_key);
            }

            // Update entities and drop old assets.
            manager.update_chunk_mesh_entities(&chunk_key, mesh.clone(), array_materials);
            if let Some(new_mesh) = mesh {
                let _drop_old_chunk_meshes = meshes.chunk_meshes.insert(chunk_key, new_mesh);
            } else {
                meshes.chunk_meshes.remove(&chunk_key);
            }
        }
    }
}
